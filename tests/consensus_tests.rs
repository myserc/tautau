use std::sync::Arc;
use libp2p::identity::Keypair;
use libp2p::PeerId;
use redb::{Database, ReadableTable};

use primetime_node::config::CONFIG;
use primetime_node::core::{self, Unit, PRIMES};
use primetime_node::do_transfer;
use primetime_node::ledger::{Event, EventType, EVENTS_TABLE, Ledger, LocalState, META_TABLE};
use primetime_node::pow::Proof;
use primetime_node::server::ServerState;
use primetime_node::smt::{self, SMT_TABLE};

// Test 1: SMT Batch Update Integrity
// - Initialize an empty redb database in a temporary directory.
// - Generate a dummy LocalState.
// - Call smt::update passing the WriteTransaction and the serialized LocalState hash.
// - Verify that exactly 256 depth nodes are written to the smt table, and a 32-byte State Root is returned.
#[test]
fn test_smt_batch_update_integrity() {
    let db_path = std::env::temp_dir().join(format!("test_smt_{}.redb", rand::random::<u64>()));
    let db = Database::create(&db_path).unwrap();

    let write_txn = db.begin_write().unwrap();
    {
        let _ = write_txn.open_table(SMT_TABLE).unwrap();
    }
    write_txn.commit().unwrap();

    let dummy_state = LocalState {
        vault_books: 1,
        counts: 10,
        prime_value: 50,
        reserved_balance: 0,
        balance_adjustment: 0,
        active_heuristic: None,
        active_book_counts: None,
        last_hash: [0u8; 32],
        nonce: 0,
    };

    let state_bytes = bincode::serialize(&dummy_state).unwrap();
    let value_hash: [u8; 32] = *blake3::hash(&state_bytes).as_bytes();
    let account_key: [u8; 32] = [42u8; 32];

    let write_txn = db.begin_write().unwrap();
    let state_root = smt::update(&write_txn, &account_key, &value_hash);
    write_txn.commit().unwrap();

    assert_eq!(state_root.len(), 32);

    let read_txn = db.begin_read().unwrap();
    let smt_table = read_txn.open_table(SMT_TABLE).unwrap();
    let count = smt_table.iter().unwrap().count();
    assert_eq!(count, 256, "Expected exactly 256 depth nodes written to SMT table");

    let _ = std::fs::remove_file(db_path);
}

// Test 2: Mathematical Scarcity & The Prime Economy
// - Assert that core::get_ordinal_for_prime correctly maps absolute values to their ordinal index.
// Example: Value 4 -> Index 1. Value 6 -> Index 2. Value 8 -> Index 3.
// - Test the conversion of "Kinetic" to "Potential" energy: Emulate a LocalState with prime_value = 1500.
// Trigger a Mint event (Threshold is 1197). Assert that the new prime_value is 303, vault_books increases by 1,
// and balance_adjustment mathematically preserves the excess.
#[test]
fn test_mathematical_scarcity_and_prime_economy() {
    // Assert ordinal index mapping
    assert_eq!(core::get_ordinal_for_prime(4), 1, "Value 4 -> Index 1");
    assert_eq!(core::get_ordinal_for_prime(6), 2, "Value 6 -> Index 2");
    assert_eq!(core::get_ordinal_for_prime(8), 3, "Value 8 -> Index 3");

    // Kinetic -> Potential energy conversion test
    let db_path = std::env::temp_dir().join(format!("test_prime_econ_{}.redb", rand::random::<u64>()));
    let ledger = Ledger::new(db_path.to_str().unwrap());

    let miner_key = Keypair::generate_ed25519();
    let miner_id = PeerId::from(miner_key.public()).to_string();
    let miner_pk = miner_key.public().encode_protobuf();

    // Emulate LocalState with prime_value = 1500
    let mut initial_state = ledger.get_local_state(&miner_id);
    initial_state.prime_value = 1500;
    initial_state.counts = core::get_ordinal_for_prime(1500);
    initial_state.vault_books = 0;
    ledger.save_local_state(&miner_id, &initial_state);

    // Create a Mint event
    let proof = Proof {
        start_hash: [0u8; 32],
        end_hash: [1u8; 32],
        iterations: 0,
        injected_txs: vec![],
    };

    let mut payload = Vec::new();
    payload.extend_from_slice(core::CHAIN_ID);
    payload.extend_from_slice(miner_id.as_bytes());
    payload.extend_from_slice(&proof.end_hash);
    let signature = miner_key.sign(&payload).unwrap();

    let mint_event = Event {
        id: proof.end_hash,
        parent_ids: vec![],
        event_type: EventType::Mint {
            proof,
            heuristic: None,
            miner: miner_id.clone(),
            miner_pk,
            signature,
        },
        entropy_delta: 0,
        state_root: [0u8; 32],
    };

    assert_eq!(CONFIG.standard_mint_scarcity, 1197);
    let success = ledger.verify_and_process_event(&mint_event);
    assert!(success, "Mint event processing should succeed");

    let updated_state = ledger.get_local_state(&miner_id);
    assert_eq!(updated_state.prime_value, 303, "New prime_value should be 303");
    assert_eq!(updated_state.vault_books, 1, "vault_books should increase by 1");

    let base_prime = if updated_state.counts > 0 { PRIMES[(updated_state.counts - 1) as usize] } else { 2 };
    assert_eq!(updated_state.balance_adjustment, 303 - base_prime, "balance_adjustment should preserve excess");

    let _ = std::fs::remove_file(db_path);
}

// Test 3: The Double-Spend & Mempool State Revert
// - Spin up an instance of the Ledger.
// - Give "Alice" an initial prime_value of 5,000.
// - Programmatically invoke the inner logic of do_transfer to send 100 Primes to "Bob".
// - Verify that Alice's memory state immediately drops to 4,900 and her nonce increments to 1 before the background loop executes.
// - Create a secondary transfer of 100 Primes to "Charlie". Verify the signature binds nonce = 2.
// - Run both transactions through ledger.verify_and_process_event.
// - Assert that Alice's final DB state is 4,800, and both Bob and Charlie have 100.
#[tokio::test]
async fn test_double_spend_and_mempool_state_revert() {
    let db_path = std::env::temp_dir().join(format!("test_double_spend_{}.redb", rand::random::<u64>()));
    let ledger = Ledger::new(db_path.to_str().unwrap());

    let alice_key = Keypair::generate_ed25519();
    let alice_id = PeerId::from(alice_key.public()).to_string();

    let bob_key = Keypair::generate_ed25519();
    let bob_id = PeerId::from(bob_key.public()).to_string();

    let charlie_key = Keypair::generate_ed25519();
    let charlie_id = PeerId::from(charlie_key.public()).to_string();

    // Give Alice initial prime_value = 5000
    let mut alice_state = ledger.get_local_state(&alice_id);
    alice_state.prime_value = 5000;
    alice_state.counts = core::get_ordinal_for_prime(5000);
    ledger.save_local_state(&alice_id, &alice_state);

    let local_state = Arc::new(tokio::sync::RwLock::new(ledger.get_local_state(&alice_id)));
    let mempool = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let (event_tx, _) = tokio::sync::mpsc::channel(100);

    let server_state = Arc::new(ServerState {
        ledger: ledger.clone(),
        event_tx,
        local_peer_id: alice_id.clone(),
        keypair: alice_key.clone(),
        local_state: local_state.clone(),
        last_sim_update: Arc::new(tokio::sync::RwLock::new(None)),
        mempool: mempool.clone(),
    });

    // Transfer 1: Send 100 Primes to Bob
    let res1 = do_transfer(bob_id.clone(), 100, Unit::Twin, &server_state).await;
    assert!(res1.is_ok(), "First transfer to Bob should succeed");

    // Verify Alice's memory state immediately drops to 4,900 and nonce = 1
    {
        let mem_state = local_state.read().await;
        assert_eq!(mem_state.prime_value, 4900, "Alice memory state should drop to 4900 immediately");
        assert_eq!(mem_state.nonce, 1, "Alice nonce should increment to 1 immediately");
    }

    // Transfer 2: Send 100 Primes to Charlie
    let res2 = do_transfer(charlie_id.clone(), 100, Unit::Twin, &server_state).await;
    assert!(res2.is_ok(), "Second transfer to Charlie should succeed");

    // Verify memory state drops to 4800 and nonce = 2
    {
        let mem_state = local_state.read().await;
        assert_eq!(mem_state.prime_value, 4800);
        assert_eq!(mem_state.nonce, 2);
    }

    // Verify signature of second tx binds nonce = 2
    let pool = mempool.lock().await;
    assert_eq!(pool.len(), 2, "Mempool should hold 2 transactions");

    let tx1_event: Event = bincode::deserialize(&pool[0]).unwrap();
    let tx2_event: Event = bincode::deserialize(&pool[1]).unwrap();

    if let EventType::Transfer { nonce, .. } = tx2_event.event_type {
        assert_eq!(nonce, 2, "Secondary transfer signature must bind nonce = 2");
    } else {
        panic!("Expected EventType::Transfer");
    }

    // Process both transactions through ledger.verify_and_process_event
    let ok1 = ledger.verify_and_process_event(&tx1_event);
    assert!(ok1, "Processing tx1 in ledger should succeed");

    let ok2 = ledger.verify_and_process_event(&tx2_event);
    assert!(ok2, "Processing tx2 in ledger should succeed");

    // Assert final DB states
    let final_alice_db = ledger.get_local_state(&alice_id);
    assert_eq!(final_alice_db.prime_value, 4800, "Alice final DB prime_value should be 4800");

    let final_bob_db = ledger.get_local_state(&bob_id);
    assert_eq!(final_bob_db.prime_value, 100, "Bob final DB prime_value should be 100");

    let final_charlie_db = ledger.get_local_state(&charlie_id);
    assert_eq!(final_charlie_db.prime_value, 100, "Charlie final DB prime_value should be 100");

    let _ = std::fs::remove_file(db_path);
}

// Test 4: Ghost Account Burn Prevention
// - Call the address structural validator on an invalid string (e.g., "hello world"). Assert that it panics or returns an Error, preventing the transaction from hitting the mempool.
// - Assert that passing Alice's own PeerId as the receiver also returns an Error (preventing self-transfers).
#[tokio::test]
async fn test_ghost_account_burn_prevention() {
    let db_path = std::env::temp_dir().join(format!("test_ghost_account_{}.redb", rand::random::<u64>()));
    let ledger = Ledger::new(db_path.to_str().unwrap());

    let alice_key = Keypair::generate_ed25519();
    let alice_id = PeerId::from(alice_key.public()).to_string();

    let mut alice_state = ledger.get_local_state(&alice_id);
    alice_state.prime_value = 5000;
    ledger.save_local_state(&alice_id, &alice_state);

    let local_state = Arc::new(tokio::sync::RwLock::new(ledger.get_local_state(&alice_id)));
    let mempool = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let (event_tx, _) = tokio::sync::mpsc::channel(100);

    let server_state = Arc::new(ServerState {
        ledger: ledger.clone(),
        event_tx,
        local_peer_id: alice_id.clone(),
        keypair: alice_key.clone(),
        local_state: local_state.clone(),
        last_sim_update: Arc::new(tokio::sync::RwLock::new(None)),
        mempool: mempool.clone(),
    });

    // 1. Invalid address string "hello world"
    let res_invalid = do_transfer("hello world".to_string(), 100, Unit::Twin, &server_state).await;
    assert!(res_invalid.is_err(), "Transfer to invalid address should return Error");

    // Verify mempool remains empty
    {
        let pool = mempool.lock().await;
        assert_eq!(pool.len(), 0, "Invalid transfer must not reach mempool");
    }

    // 2. Self transfer to Alice's own PeerId
    let res_self = do_transfer(alice_id.clone(), 100, Unit::Twin, &server_state).await;
    assert!(res_self.is_err(), "Transfer to self should return Error");

    // Verify mempool remains empty
    {
        let pool = mempool.lock().await;
        assert_eq!(pool.len(), 0, "Self transfer must not reach mempool");
    }

    let _ = std::fs::remove_file(db_path);
}

// Test 5: DAG Pruning
// - Insert 1,050 dummy events into the EVENTS_TABLE and META_TABLE in redb.
// - Call ledger.prune_events(50).
// - Assert that the events from index 0 to 50 are physically missing from the DB, but events 51 to 1050 remain intact.
#[test]
fn test_dag_pruning() {
    let db_path = std::env::temp_dir().join(format!("test_dag_pruning_{}.redb", rand::random::<u64>()));
    let ledger = Ledger::new(db_path.to_str().unwrap());

    // Insert 1,050 dummy events (index 0 to 1050)
    let tx = ledger.db.begin_write().unwrap();
    {
        let mut events_table = tx.open_table(EVENTS_TABLE).unwrap();
        let mut meta_table = tx.open_table(META_TABLE).unwrap();

        for i in 0..=1050u64 {
            let mut id = [0u8; 32];
            id[0..8].copy_from_slice(&i.to_le_bytes());

            let event = Event {
                id,
                parent_ids: vec![],
                event_type: EventType::Tick {
                    proof: Proof {
                        start_hash: [0u8; 32],
                        end_hash: [0u8; 32],
                        iterations: 0,
                        injected_txs: vec![],
                    },
                    miner: "dummy_miner".to_string(),
                    miner_pk: vec![],
                    signature: vec![],
                },
                entropy_delta: 0,
                state_root: [0u8; 32],
            };

            let serialized = bincode::serialize(&event).unwrap();
            events_table.insert(&id, serialized.as_slice()).unwrap();
            meta_table.insert(format!("log_{}", i).as_str(), id.as_slice()).unwrap();
        }
    }
    tx.commit().unwrap();

    // Call ledger.prune_events(50)
    ledger.prune_events(50);

    // Assert index 0 to 50 missing, 51 to 1050 intact
    let read_tx = ledger.db.begin_read().unwrap();
    let events_table = read_tx.open_table(EVENTS_TABLE).unwrap();
    let meta_table = read_tx.open_table(META_TABLE).unwrap();

    for i in 0..=50u64 {
        let mut id = [0u8; 32];
        id[0..8].copy_from_slice(&i.to_le_bytes());

        let log_key = format!("log_{}", i);
        assert!(meta_table.get(log_key.as_str()).unwrap().is_none(), "log_{} should be pruned from META_TABLE", i);
        assert!(events_table.get(&id).unwrap().is_none(), "Event {} should be pruned from EVENTS_TABLE", i);
    }

    for i in 51..=1050u64 {
        let mut id = [0u8; 32];
        id[0..8].copy_from_slice(&i.to_le_bytes());

        let log_key = format!("log_{}", i);
        assert!(meta_table.get(log_key.as_str()).unwrap().is_some(), "log_{} should remain in META_TABLE", i);
        assert!(events_table.get(&id).unwrap().is_some(), "Event {} should remain in EVENTS_TABLE", i);
    }

    let _ = std::fs::remove_file(db_path);
}
