/*!
 * PrimeTime Arithmodynamic Ledger Engine
 *
 * A mobile-native, stateless DAG ledger anchored in mathematical scarcity.
 * Features Clockwork Proof-of-Work, 256-depth SMTs via Redb, and zero-sum Prime-pegged value physics.
 */

#![allow(clippy::type_complexity)]

pub mod utils {
    pub mod serde_hex {
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(data: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(&hex::encode(data))
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
            let s = String::deserialize(d)?;
            let decoded = hex::decode(&s).map_err(serde::de::Error::custom)?;
            if decoded.len() != 32 {
                return Err(serde::de::Error::custom("Hash must be exactly 32 bytes"));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&decoded);
            Ok(arr)
        }
    }

    pub mod serde_hex_vec {
        use serde::{Deserialize, Deserializer, Serializer, ser::SerializeSeq};
        pub fn serialize<S: Serializer>(data: &Vec<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
            let mut seq = s.serialize_seq(Some(data.len()))?;
            for e in data {
                seq.serialize_element(&hex::encode(e))?;
            }
            seq.end()
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<[u8; 32]>, D::Error> {
            let vec_s = Vec::<String>::deserialize(d)?;
            let mut res = Vec::new();
            for s in vec_s {
                let decoded = hex::decode(&s).map_err(serde::de::Error::custom)?;
                if decoded.len() != 32 {
                    return Err(serde::de::Error::custom("Hash must be exactly 32 bytes"));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&decoded);
                res.push(arr);
            }
            Ok(res)
        }
    }
}

pub mod config {
    use crate::core::Unit;
    use std::sync::LazyLock;

    pub struct Config {
        pub mode: String,
        pub limit: usize,
        pub total_book_counts: u64,
        pub standard_mint_scarcity: u64,
        pub units: Vec<Unit>,
    }

    pub static CONFIG: LazyLock<Config> = LazyLock::new(|| Config {
        mode: "COM".to_string(),
        limit: 200_000,
        total_book_counts: 1_000,
        standard_mint_scarcity: 1_197,
        units: vec![Unit::Day, Unit::Degree, Unit::Twin],
    });
}

pub mod core {
    use crate::config::CONFIG;
    use std::collections::HashMap;
    use std::sync::LazyLock;

    pub const CHAIN_ID: &[u8] = b"primetime-mainnet-v2";

    // The Engine's Mathematical Ground Truth
    pub static PRIMES: LazyLock<Vec<u64>> = LazyLock::new(|| generate_composites(CONFIG.limit));

    fn generate_composites(limit: usize) -> Vec<u64> {
        let mut sieve = vec![true; limit];
        let sqrt_limit = (limit as f64).sqrt() as usize;
        for p in 2..=sqrt_limit {
            if sieve[p] {
                let mut i = p * p;
                while i < limit {
                    sieve[i] = false;
                    i += p;
                }
            }
        }
        let mut composites = Vec::with_capacity(limit);
        for i in 2..limit {
            if !sieve[i] {
                composites.push(i as u64);
            }
        }
        composites
    }

    pub fn get_ordinal_for_prime(value: u64) -> u64 {
        PRIMES.partition_point(|&x| x <= value) as u64
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub enum Unit { Quadrant, Day, Degree, Minute, Twin }

    impl Unit {
        pub fn counts(&self) -> u64 {
            if CONFIG.mode == "COM" {
                match self {
                    Unit::Quadrant => 0, Unit::Day => 50, Unit::Degree => 10, Unit::Minute => 0, Unit::Twin => 1,
                }
            } else {
                match self {
                    Unit::Quadrant => 162_000, Unit::Day => 43_200, Unit::Degree => 1_800, Unit::Minute => 30, Unit::Twin => 1,
                }
            }
        }
        pub fn all() -> Vec<Unit> { CONFIG.units.clone() }
    }

    pub struct HeuristicStandard {
        pub mint_scarcity: u64,
        pub mint_counts: u64,
        pub precedent: u64,
    }

    pub static STANDARDS: LazyLock<HashMap<Unit, HeuristicStandard>> = LazyLock::new(|| {
        let mut map = HashMap::new();
        let _ = &*PRIMES;
        for unit in Unit::all() {
            let precedent = PRIMES[(unit.counts() - 1) as usize];
            let mint_scarcity = (CONFIG.standard_mint_scarcity / precedent) * precedent;
            let mint_counts = get_ordinal_for_prime(mint_scarcity) + 1;
            map.insert(unit, HeuristicStandard { mint_scarcity, mint_counts, precedent });
        }
        map
    });
}

pub mod pow {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Proof {
        #[serde(with = "crate::utils::serde_hex")]
        pub start_hash: [u8; 32],
        #[serde(with = "crate::utils::serde_hex")]
        pub end_hash: [u8; 32],
        pub iterations: u64,
        pub injected_txs: Vec<(u64, Vec<u8>)>,
    }

    pub fn mine_iterations(last_hash: &[u8; 32], miner_id: &str, target_iterations: u64, pending_events: Vec<Vec<u8>>) -> Proof {
        // Replay Attack protection: Bind proof to Miner ID
        let mut hasher = blake3::Hasher::new();
        hasher.update(last_hash);
        hasher.update(miner_id.as_bytes());
        let true_start_hash: [u8; 32] = hasher.finalize().into();

        let mut current_bytes = true_start_hash;
        let mut injected_txs = Vec::new();
        let num_events = pending_events.len() as u64;
        let events_per_iteration = if num_events == 0 { 0 } else { (target_iterations / num_events).max(1) };
        let mut event_idx = 0;

        for i in 0..target_iterations {
            current_bytes = blake3::hash(&current_bytes).into();

            while event_idx < num_events && (i % events_per_iteration == 0 || i == target_iterations - 1) {
                let tx_bytes = &pending_events[event_idx as usize];
                let mut hasher = blake3::Hasher::new();
                hasher.update(&current_bytes);
                hasher.update(tx_bytes);
                current_bytes = hasher.finalize().into();
                injected_txs.push((i, tx_bytes.clone()));
                event_idx += 1;
                break;
            }

            if i == target_iterations - 1 {
                while event_idx < num_events {
                    let tx_bytes = &pending_events[event_idx as usize];
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(&current_bytes);
                    hasher.update(tx_bytes);
                    current_bytes = hasher.finalize().into();
                    injected_txs.push((i, tx_bytes.clone()));
                    event_idx += 1;
                }
            }
        }

        Proof {
            start_hash: *last_hash,
            end_hash: current_bytes,
            iterations: target_iterations,
            injected_txs,
        }
    }

    pub fn verify_proof(proof: &Proof, miner_id: &str) -> bool {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&proof.start_hash);
        hasher.update(miner_id.as_bytes());
        let mut current_bytes: [u8; 32] = hasher.finalize().into();

        let mut inject_idx = 0;
        for i in 0..proof.iterations {
            current_bytes = blake3::hash(&current_bytes).into();
            while inject_idx < proof.injected_txs.len() && proof.injected_txs[inject_idx].0 == i {
                let mut hasher = blake3::Hasher::new();
                hasher.update(&current_bytes);
                hasher.update(&proof.injected_txs[inject_idx].1);
                current_bytes = hasher.finalize().into();
                inject_idx += 1;
            }
        }
        current_bytes == proof.end_hash
    }
}

pub mod smt {
    use redb::{TableDefinition, WriteTransaction, ReadableTable};
    use std::sync::LazyLock;

    pub const DEPTH: usize = 256;
    pub const SMT_TABLE: TableDefinition<&str, &[u8; 32]> = TableDefinition::new("smt");

    pub static EMPTY_HASHES: LazyLock<Vec<[u8; 32]>> = LazyLock::new(|| {
        let mut hashes = Vec::with_capacity(DEPTH + 1);
        let mut current = [0u8; 32];
        hashes.push(current);
        for _ in 0..DEPTH {
            let mut h = blake3::Hasher::new();
            h.update(&current);
            h.update(&current);
            current = *h.finalize().as_bytes();
            hashes.push(current);
        }
        hashes
    });

    pub fn update(tx: &WriteTransaction, key: &[u8; 32], value_hash: &[u8; 32]) -> [u8; 32] {
        let mut current_hash = *value_hash;
        let mut smt_table = tx.open_table(SMT_TABLE).unwrap();

        for level in 0..DEPTH {
            let byte_idx = level / 8;
            let bit_idx = level % 8;
            let is_right = (key[byte_idx] >> bit_idx) & 1 == 1;

            let sibling_node_key = format!("smt_{}_{:?}", level, &key[0..4]);
            let sibling_hash = if let Ok(Some(bytes)) = smt_table.get(sibling_node_key.as_str()) {
                *bytes.value()
            } else {
                EMPTY_HASHES[level]
            };

            let mut hasher = blake3::Hasher::new();
            if is_right {
                hasher.update(&sibling_hash);
                hasher.update(&current_hash);
            } else {
                hasher.update(&current_hash);
                hasher.update(&sibling_hash);
            }
            current_hash = *hasher.finalize().as_bytes();

            let node_key = format!("smt_{}_{:?}", level + 1, &key[0..4]);
            smt_table.insert(node_key.as_str(), &current_hash).unwrap();
        }
        current_hash
    }
}

pub mod ledger {
    use crate::config::CONFIG;
    use crate::core::{CHAIN_ID, PRIMES, Unit, get_ordinal_for_prime};
    use crate::pow::Proof;
    use crate::smt::update;
    use redb::{Database, TableDefinition, WriteTransaction, ReadableTable};
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    pub const EVENTS_TABLE: TableDefinition<&[u8; 32], &[u8]> = TableDefinition::new("events");
    pub const TRANSFERS_TABLE: TableDefinition<[u8; 8], &[u8]> = TableDefinition::new("transfers");
    pub const STATE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("state");
    pub const META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub enum EventType {
        Mint { proof: Proof, heuristic: Option<Unit>, miner: String, miner_pk: Vec<u8>, signature: Vec<u8> },
        Tick { proof: Proof, miner: String, miner_pk: Vec<u8>, signature: Vec<u8> },
        Transfer { sender: String, sender_pk: Vec<u8>, receiver: String, amount_prime: u64, heuristic: Unit, nonce: u64, signature: Vec<u8> },
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Event {
        #[serde(with = "crate::utils::serde_hex")] pub id: [u8; 32],
        #[serde(with = "crate::utils::serde_hex_vec")] pub parent_ids: Vec<[u8; 32]>,
        pub event_type: EventType,
        pub entropy_delta: i64,
        #[serde(with = "crate::utils::serde_hex")] pub state_root: [u8; 32],
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct LocalState {
        pub vault_books: u64,
        pub counts: u64,
        pub prime_value: u64,
        pub reserved_balance: u64,
        pub balance_adjustment: u64,
        pub active_heuristic: Option<Unit>,
        pub active_book_counts: Option<u64>,
        #[serde(with = "crate::utils::serde_hex")] pub last_hash: [u8; 32],
        pub nonce: u64,
    }

    pub struct Ledger {
        pub db: Arc<Database>,
        pub net_entropy: std::sync::atomic::AtomicI64,
        pub surplus_events: std::sync::atomic::AtomicI64,
        pub void_events: std::sync::atomic::AtomicI64,
        pub state_root: std::sync::RwLock<[u8; 32]>,
    }

    impl Ledger {
        pub fn new(path: &str) -> Arc<Self> {
            let db = Database::create(path).expect("Failed to open redb database");
            let write_txn = db.begin_write().unwrap();
            {
                let _ = write_txn.open_table(EVENTS_TABLE).unwrap();
                let _ = write_txn.open_table(TRANSFERS_TABLE).unwrap();
                let _ = write_txn.open_table(STATE_TABLE).unwrap();
                let _ = write_txn.open_table(META_TABLE).unwrap();
                let _ = write_txn.open_table(crate::smt::SMT_TABLE).unwrap();
            }
            write_txn.commit().unwrap();

            let read_txn = db.begin_read().unwrap();
            let mut entropy = 0; let mut surplus = 0; let mut voids = 0; let mut sr = [0u8; 32];
            if let Ok(meta_table) = read_txn.open_table(META_TABLE) {
                if let Ok(Some(ent)) = meta_table.get("net_entropy") { entropy = i64::from_le_bytes(ent.value().try_into().unwrap_or([0; 8])); }
                if let Ok(Some(s)) = meta_table.get("surplus") { surplus = i64::from_le_bytes(s.value().try_into().unwrap_or([0; 8])); }
                if let Ok(Some(v)) = meta_table.get("voids") { voids = i64::from_le_bytes(v.value().try_into().unwrap_or([0; 8])); }
                if let Ok(Some(r)) = meta_table.get("state_root") {
                    let val = r.value();
                    if val.len() == 32 { sr.copy_from_slice(val); }
                }
            }

            Arc::new(Ledger {
                db: Arc::new(db),
                net_entropy: std::sync::atomic::AtomicI64::new(entropy),
                surplus_events: std::sync::atomic::AtomicI64::new(surplus),
                void_events: std::sync::atomic::AtomicI64::new(voids),
                state_root: std::sync::RwLock::new(sr),
            })
        }

        pub fn get_or_create_keypair(&self) -> libp2p::identity::Keypair {
            let read_txn = self.db.begin_read().unwrap();
            if let Ok(meta_table) = read_txn.open_table(META_TABLE) {
                if let Ok(Some(bytes)) = meta_table.get("local_node_key") {
                    if let Ok(keypair) = libp2p::identity::Keypair::from_protobuf_encoding(bytes.value()) {
                        return keypair;
                    }
                }
            }
            let new_key = libp2p::identity::Keypair::generate_ed25519();
            let write_txn = self.db.begin_write().unwrap();
            {
                let mut meta_table = write_txn.open_table(META_TABLE).unwrap();
                meta_table.insert("local_node_key", new_key.to_protobuf_encoding().unwrap().as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
            new_key
        }

        pub fn get_event_count(&self) -> u64 {
            let read_txn = self.db.begin_read().unwrap();
            if let Ok(meta_table) = read_txn.open_table(META_TABLE) {
                if let Ok(Some(val)) = meta_table.get("event_count") {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(val.value());
                    return u64::from_le_bytes(buf);
                }
            }
            0
        }

        pub fn get_events_after(&self, from_count: u64) -> Vec<Event> {
            let mut events = Vec::new();
            let current_max = self.get_event_count();
            let target_max = current_max.min(from_count + 500);

            let read_txn = self.db.begin_read().unwrap();
            if let (Ok(meta_table), Ok(events_table)) = (read_txn.open_table(META_TABLE), read_txn.open_table(EVENTS_TABLE)) {
                for i in (from_count + 1)..=target_max {
                    let log_key = format!("log_{}", i);
                    if let Ok(Some(hash_bytes)) = meta_table.get(log_key.as_str()) {
                        let mut hash = [0u8; 32];
                        hash.copy_from_slice(hash_bytes.value());
                        if let Ok(Some(event_bytes)) = events_table.get(&hash) {
                            if let Ok(event) = bincode::deserialize::<Event>(event_bytes.value()) {
                                events.push(event);
                            }
                        }
                    }
                }
            }
            events
        }

        fn increment_counter(&self, tx: &WriteTransaction, key: &str) -> u64 {
            let mut meta_table = tx.open_table(META_TABLE).unwrap();
            let mut current = 0;
            if let Ok(Some(val)) = meta_table.get(key) {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(val.value());
                current = u64::from_le_bytes(buf);
            }
            meta_table.insert(key, (current + 1).to_le_bytes().as_slice()).unwrap();
            current
        }

        fn write_event(&self, tx: &WriteTransaction, event: &Event) {
            let serialized = bincode::serialize(event).unwrap();
            {
                let mut events_table = tx.open_table(EVENTS_TABLE).unwrap();
                events_table.insert(&event.id, serialized.as_slice()).unwrap();
            }

            let idx = self.increment_counter(tx, "event_count");
            {
                let mut meta_table = tx.open_table(META_TABLE).unwrap();
                meta_table.insert(format!("log_{}", idx).as_str(), event.id.as_slice()).unwrap();
            }

            if matches!(event.event_type, EventType::Transfer { .. }) {
                let t_idx = self.increment_counter(tx, "transfer_idx");
                let mut transfers_table = tx.open_table(TRANSFERS_TABLE).unwrap();
                transfers_table.insert(t_idx.to_be_bytes(), serialized.as_slice()).unwrap();
                if t_idx > 100 { let _ = transfers_table.remove((t_idx - 100).to_be_bytes()); }
            }

            let old_entropy = self.net_entropy.fetch_add(event.entropy_delta, std::sync::atomic::Ordering::SeqCst);
            let new_entropy = old_entropy + event.entropy_delta;
            let tbc = CONFIG.total_book_counts as i64;
            if new_entropy >= tbc {
                self.net_entropy.fetch_sub(tbc, std::sync::atomic::Ordering::SeqCst);
                self.surplus_events.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            } else if new_entropy <= -tbc {
                self.net_entropy.fetch_add(tbc, std::sync::atomic::Ordering::SeqCst);
                self.void_events.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }

            let mut meta_table = tx.open_table(META_TABLE).unwrap();
            meta_table.insert("net_entropy", self.net_entropy.load(std::sync::atomic::Ordering::SeqCst).to_le_bytes().as_slice()).unwrap();
            meta_table.insert("surplus", self.surplus_events.load(std::sync::atomic::Ordering::SeqCst).to_le_bytes().as_slice()).unwrap();
            meta_table.insert("voids", self.void_events.load(std::sync::atomic::Ordering::SeqCst).to_le_bytes().as_slice()).unwrap();
        }

        fn update_smt_for_account(&self, tx: &WriteTransaction, account_id: &str, state: &LocalState) -> [u8; 32] {
            let mut key_hasher = blake3::Hasher::new();
            key_hasher.update(account_id.as_bytes());
            let account_key: [u8; 32] = key_hasher.finalize().into();

            let state_bytes = bincode::serialize(state).unwrap();
            let leaf_value: [u8; 32] = *blake3::hash(&state_bytes).as_bytes();

            let new_root = update(tx, &account_key, &leaf_value);

            *self.state_root.write().unwrap() = new_root;
            let mut meta_table = tx.open_table(META_TABLE).unwrap();
            meta_table.insert("state_root", new_root.as_slice()).unwrap();

            new_root
        }

        fn process_event_internal(&self, tx: &WriteTransaction, event: &Event) -> bool {
            {
                let events_table = tx.open_table(EVENTS_TABLE).unwrap();
                if events_table.get(&event.id).unwrap().is_some() { return false; }
            }

            match &event.event_type {
                EventType::Mint { proof, miner, miner_pk, signature, .. }
                | EventType::Tick { proof, miner, miner_pk, signature } => {
                    if proof.start_hash != [0u8; 32] {
                        let events_table = tx.open_table(EVENTS_TABLE).unwrap();
                        if events_table.get(&proof.start_hash).unwrap().is_none() { return false; }
                    }

                    if proof.iterations > 0 && !crate::pow::verify_proof(proof, miner) { return false; }

                    use libp2p::identity::PublicKey;
                    if let Ok(pk) = PublicKey::try_decode_protobuf(miner_pk) {
                        let mut payload = Vec::new();
                        payload.extend_from_slice(CHAIN_ID);
                        payload.extend_from_slice(miner.as_bytes());
                        payload.extend_from_slice(&proof.end_hash);
                        if !pk.verify(&payload, signature) { return false; }
                    } else { return false; }

                    let mut miner_state = Self::read_local_state(tx, miner);
                    miner_state.last_hash = event.id;

                    // Network-Verified Rollover
                    if miner_state.active_book_counts.unwrap_or(0) == 0 && miner_state.vault_books > 0 {
                        miner_state.vault_books -= 1;
                        miner_state.active_book_counts = Some(CONFIG.total_book_counts as u64);
                    }

                    // Burn Fuel
                    if let Some(ref mut fuel) = miner_state.active_book_counts {
                        *fuel = fuel.saturating_sub(1);
                    }

                    // Flawless Minting & Wealth Preservation
                    if let EventType::Mint { .. } = &event.event_type {
                        if miner_state.prime_value < CONFIG.standard_mint_scarcity { return false; }
                        miner_state.vault_books += 1;
                        miner_state.prime_value = miner_state.prime_value.saturating_sub(CONFIG.standard_mint_scarcity);
                        miner_state.counts = crate::core::get_ordinal_for_prime(miner_state.prime_value);
                        let base_prime = if miner_state.counts > 0 { crate::core::PRIMES[(miner_state.counts - 1) as usize] } else { 2 };
                        miner_state.balance_adjustment = miner_state.prime_value.saturating_sub(base_prime);
                        miner_state.active_heuristic = None;
                    } else {
                        miner_state.counts += 1;
                        miner_state.update_prime_value();
                    }

                    Self::write_local_state(tx, miner, &miner_state);
                    self.update_smt_for_account(tx, miner, &miner_state);
                    self.write_event(tx, event);

                    for (_, tx_bytes) in &proof.injected_txs {
                        if let Ok(tx_event) = bincode::deserialize::<Event>(tx_bytes) {
                            if matches!(tx_event.event_type, EventType::Transfer { .. }) {
                                self.process_event_internal(tx, &tx_event);
                            }
                        }
                    }
                    true
                }
                EventType::Transfer { sender, sender_pk, receiver, amount_prime, nonce, signature, .. } => {
                    use libp2p::identity::PublicKey;
                    if let Ok(pk) = PublicKey::try_decode_protobuf(sender_pk) {
                        let mut payload = Vec::new();
                        payload.extend_from_slice(CHAIN_ID);
                        payload.extend_from_slice(sender.as_bytes());
                        payload.extend_from_slice(receiver.as_bytes());
                        payload.extend_from_slice(&amount_prime.to_le_bytes());
                        payload.extend_from_slice(&nonce.to_le_bytes());
                        if !pk.verify(&payload, signature) { return false; }
                    } else { return false; }

                    let mut sender_state = Self::read_local_state(tx, sender);
                    if *nonce != sender_state.nonce + 1 || (sender_state.prime_value.saturating_sub(sender_state.reserved_balance)) < *amount_prime {
                        return false;
                    }

                    sender_state.prime_value -= amount_prime;
                    sender_state.counts = get_ordinal_for_prime(sender_state.prime_value);
                    let sender_base = if sender_state.counts > 0 { PRIMES[(sender_state.counts - 1) as usize] } else { 2 };
                    sender_state.balance_adjustment = sender_state.prime_value.saturating_sub(sender_base);
                    sender_state.nonce = *nonce;
                    Self::write_local_state(tx, sender, &sender_state);
                    self.update_smt_for_account(tx, sender, &sender_state);

                    let mut receiver_state = Self::read_local_state(tx, receiver);
                    receiver_state.prime_value += amount_prime;
                    receiver_state.counts = get_ordinal_for_prime(receiver_state.prime_value);
                    let receiver_base = if receiver_state.counts > 0 { PRIMES[(receiver_state.counts - 1) as usize] } else { 2 };
                    receiver_state.balance_adjustment = receiver_state.prime_value.saturating_sub(receiver_base);
                    Self::write_local_state(tx, receiver, &receiver_state);
                    self.update_smt_for_account(tx, receiver, &receiver_state);

                    self.write_event(tx, event);
                    true
                }
            }
        }

        pub fn verify_and_process_event(&self, event: &Event) -> bool {
            let tx = self.db.begin_write().unwrap();
            let success = self.process_event_internal(&tx, event);
            if success { tx.commit().unwrap(); true } else { tx.abort().unwrap(); false }
        }

        pub fn get_recent_transfers(&self, limit: usize) -> Vec<Event> {
            let mut events = Vec::new();
            let read_txn = self.db.begin_read().unwrap();
            if let Ok(transfers_table) = read_txn.open_table(TRANSFERS_TABLE) {
                if let Ok(iter) = transfers_table.iter() {
                    for res in iter.rev().take(limit) {
                        if let Ok((_, val)) = res {
                            if let Ok(e) = bincode::deserialize::<Event>(val.value()) { events.push(e); }
                        }
                    }
                }
            }
            events
        }

        pub fn prune_events(&self, up_to_tick: u64) {
            if let Ok(tx) = self.db.begin_write() {
                if let (Ok(mut events_table), Ok(mut meta_table)) = (tx.open_table(EVENTS_TABLE), tx.open_table(META_TABLE)) {
                    for i in 0..=up_to_tick {
                        let log_key = format!("log_{}", i);
                        let mut hash_to_remove = None;
                        if let Ok(Some(hash_bytes)) = meta_table.get(log_key.as_str()) {
                            let mut hash = [0u8; 32];
                            hash.copy_from_slice(hash_bytes.value());
                            hash_to_remove = Some(hash);
                        }
                        if let Some(hash) = hash_to_remove {
                            let _ = events_table.remove(&hash);
                            let _ = meta_table.remove(log_key.as_str());
                        }
                    }
                }
                let _ = tx.commit();
            }
        }

        pub fn get_local_state(&self, id: &str) -> LocalState {
            let read_txn = self.db.begin_read().unwrap();
            if let Ok(state_table) = read_txn.open_table(STATE_TABLE) {
                if let Ok(Some(val)) = state_table.get(id) {
                    return bincode::deserialize(val.value()).unwrap_or_else(|_| Self::default_state());
                }
            }
            Self::default_state()
        }

        fn read_local_state(tx: &WriteTransaction, id: &str) -> LocalState {
            if let Ok(state_table) = tx.open_table(STATE_TABLE) {
                if let Ok(Some(val)) = state_table.get(id) {
                    return bincode::deserialize(val.value()).unwrap_or_else(|_| Self::default_state());
                }
            }
            Self::default_state()
        }

        pub fn save_local_state(&self, id: &str, state: &LocalState) {
            let write_txn = self.db.begin_write().unwrap();
            Self::write_local_state(&write_txn, id, state);
            write_txn.commit().unwrap();
        }

        fn write_local_state(tx: &WriteTransaction, id: &str, state: &LocalState) {
            let serialized = bincode::serialize(state).unwrap();
            let mut state_table = tx.open_table(STATE_TABLE).unwrap();
            state_table.insert(id, serialized.as_slice()).unwrap();
        }

        pub fn default_state() -> LocalState {
            LocalState { vault_books: 0, counts: 0, prime_value: 0, reserved_balance: 0, balance_adjustment: 0, active_heuristic: None, active_book_counts: None, last_hash: [0u8; 32], nonce: 0 }
        }
    }

    impl LocalState {
        pub fn update_prime_value(&mut self) {
            let mut ordinal_idx = if self.counts > 0 { self.counts - 1 } else { 0 };
            ordinal_idx = ordinal_idx.min(PRIMES.len() as u64 - 1);
            self.prime_value = PRIMES[ordinal_idx as usize] + self.balance_adjustment;
        }
    }
}

pub mod p2p {
    use crate::ledger::Event;
    use libp2p::identity::Keypair;
    use libp2p::kad::store::MemoryStore;
    use libp2p::{PeerId, Swarm, autonat, dcutr, gossipsub, identify, kad, mdns, noise, ping, relay, request_response, swarm::NetworkBehaviour, tcp, yamux};
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "P2PBehaviourEvent")]
    pub struct P2PBehaviour {
        pub gossipsub: gossipsub::Behaviour,
        pub kademlia: kad::Behaviour<MemoryStore>,
        pub sync_req_resp: request_response::cbor::Behaviour<SyncRequest, SyncResponse>,
        pub mdns: mdns::tokio::Behaviour,
        pub autonat: autonat::Behaviour,
        pub relay_client: relay::client::Behaviour,
        pub dcutr: dcutr::Behaviour,
        pub identify: identify::Behaviour,
        pub ping: ping::Behaviour,
    }

    #[derive(Debug)]
    pub enum P2PBehaviourEvent {
        Gossipsub(gossipsub::Event),
        Kademlia(kad::Event),
        SyncReqResp(request_response::Event<SyncRequest, SyncResponse>),
        Mdns(mdns::Event),
        Autonat(autonat::Event),
        RelayClient(relay::client::Event),
        Dcutr(dcutr::Event),
        Identify(identify::Event),
        Ping(ping::Event),
    }

    impl From<gossipsub::Event> for P2PBehaviourEvent { fn from(e: gossipsub::Event) -> Self { Self::Gossipsub(e) } }
    impl From<kad::Event> for P2PBehaviourEvent { fn from(e: kad::Event) -> Self { Self::Kademlia(e) } }
    impl From<request_response::Event<SyncRequest, SyncResponse>> for P2PBehaviourEvent { fn from(e: request_response::Event<SyncRequest, SyncResponse>) -> Self { Self::SyncReqResp(e) } }
    impl From<mdns::Event> for P2PBehaviourEvent { fn from(e: mdns::Event) -> Self { Self::Mdns(e) } }
    impl From<autonat::Event> for P2PBehaviourEvent { fn from(e: autonat::Event) -> Self { Self::Autonat(e) } }
    impl From<relay::client::Event> for P2PBehaviourEvent { fn from(e: relay::client::Event) -> Self { Self::RelayClient(e) } }
    impl From<dcutr::Event> for P2PBehaviourEvent { fn from(e: dcutr::Event) -> Self { Self::Dcutr(e) } }
    impl From<identify::Event> for P2PBehaviourEvent { fn from(e: identify::Event) -> Self { Self::Identify(e) } }
    impl From<ping::Event> for P2PBehaviourEvent { fn from(e: ping::Event) -> Self { Self::Ping(e) } }

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct SyncRequest { pub from_tick: u64 }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SyncResponse { pub events: Vec<Event> }

    pub struct P2PNode {
        pub swarm: Swarm<P2PBehaviour>,
        pub peer_id: PeerId,
        pub keypair: Keypair,
    }

    impl P2PNode {
        pub fn new(local_key: Keypair) -> Result<Self, Box<dyn std::error::Error>> {
            let local_peer_id = PeerId::from(local_key.public());
            let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
                .with_tokio()
                .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
                .with_relay_client(noise::Config::new, yamux::Config::default)?
                .with_behaviour(|key, relay_client| {
                    let gossip_config = gossipsub::ConfigBuilder::default()
                        .heartbeat_interval(Duration::from_secs(10))
                        .validation_mode(gossipsub::ValidationMode::Permissive).build().unwrap();
                    let mut gossipsub = gossipsub::Behaviour::new(gossipsub::MessageAuthenticity::Signed(key.clone()), gossip_config).unwrap();
                    gossipsub.subscribe(&gossipsub::IdentTopic::new("primetime-dag")).unwrap();

                    let store = MemoryStore::new(local_peer_id);
                    let kademlia = kad::Behaviour::new(local_peer_id, store);
                    let sync_req_resp = request_response::cbor::Behaviour::new(
                        [(libp2p::StreamProtocol::new("/primetime/sync/1.0.0"), request_response::ProtocolSupport::Full)],
                        request_response::Config::default(),
                    );
                    let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id).unwrap();
                    let autonat = autonat::Behaviour::new(local_peer_id, autonat::Config::default());
                    let dcutr = dcutr::Behaviour::new(local_peer_id);
                    let identify = identify::Behaviour::new(identify::Config::new("/primetime/2.0.0".into(), key.public()));
                    let ping = ping::Behaviour::new(ping::Config::default());

                    P2PBehaviour { gossipsub, kademlia, sync_req_resp, mdns, autonat, relay_client, dcutr, identify, ping }
                })?
                .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                .build();

            Ok(P2PNode { swarm, peer_id: local_peer_id, keypair: local_key })
        }
    }
}

pub mod server {
    use crate::core::Unit;
    use crate::ledger::{Event, Ledger};
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct SimUpdate {
        pub tick: u64,
        pub node_id: String,
        pub net_entropy: i64,
        pub void_events: i64,
        pub surplus_events: i64,
        pub peer_count: usize,
        pub local_vault_books: u64,
        pub local_prime_value: u64,
        pub local_counts: u64,
        pub local_nonce: u64,
        pub unit_precedents: std::collections::HashMap<String, u64>,
        pub hash_rate: u64,
        pub active_heuristic: Option<Unit>,
        pub recent_transfers: Vec<Event>,
        pub external_addrs: Vec<String>,
        pub nat_status: String,
        pub state_root: String,
        pub eco_mode_active: bool,
    }

    pub struct ServerState {
        pub ledger: Arc<Ledger>,
        pub event_tx: tokio::sync::mpsc::Sender<Event>,
        pub local_peer_id: String,
        pub keypair: libp2p::identity::Keypair,
        pub local_state: Arc<tokio::sync::RwLock<crate::ledger::LocalState>>,
        pub last_sim_update: Arc<tokio::sync::RwLock<Option<SimUpdate>>>,
        pub mempool: Arc<tokio::sync::Mutex<Vec<Vec<u8>>>>,
    }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use serde::Deserialize;
use crate::core::CHAIN_ID;
use crate::ledger::{Event, EventType};
use crate::server::{ServerState, SimUpdate};

static IS_MINING: AtomicBool = AtomicBool::new(true);
static ECO_MODE: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
pub struct EncryptPayload { pub receiver_pub_key: String, pub plaintext: String }

#[derive(Deserialize)]
pub struct DecryptPayload { pub ciphertext_base64: String }

pub async fn encrypt_qr_payload(payload: EncryptPayload) -> Result<String, String> {
    use aes_gcm::{Aes256Gcm, Key, Nonce, aead::{Aead, KeyInit}};
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use curve25519_dalek::edwards::CompressedEdwardsY;
    use libp2p::PeerId;
    use libp2p::identity::PublicKey as Libp2pPublicKey;
    use rand::RngCore;
    use std::str::FromStr;
    use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

    let peer_id = PeerId::from_str(&payload.receiver_pub_key).map_err(|_| "Invalid PeerId")?;
    let peer_bytes = peer_id.to_bytes();
    if peer_bytes.len() < 2 || peer_bytes[0] != 0x00 { return Err("PeerId missing identity hash".to_string()); }

    let pubkey_bytes = &peer_bytes[2..];
    let pubkey = Libp2pPublicKey::try_decode_protobuf(pubkey_bytes).map_err(|_| "Failed to decode PublicKey")?;
    let ed25519_pub = pubkey.try_into_ed25519().map_err(|_| "Not an Ed25519 key")?;

    let ed_point = CompressedEdwardsY(ed25519_pub.to_bytes()).decompress().ok_or("Invalid Edwards point")?;
    let x25519_pub_bytes = ed_point.to_montgomery().to_bytes();
    let receiver_x25519_pub = X25519PublicKey::from(x25519_pub_bytes);

    let mut secret_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret_bytes);
    let ephemeral_secret = StaticSecret::from(secret_bytes);
    let ephemeral_pub = X25519PublicKey::from(&ephemeral_secret);

    let shared_secret = ephemeral_secret.diffie_hellman(&receiver_x25519_pub);
    let cipher_key = Key::<Aes256Gcm>::from_slice(shared_secret.as_bytes());
    let cipher = Aes256Gcm::new(cipher_key);

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, payload.plaintext.as_bytes()).map_err(|_| "AES encryption failed")?;

    let mut final_payload = Vec::new();
    final_payload.extend_from_slice(ephemeral_pub.as_bytes());
    final_payload.extend_from_slice(&nonce_bytes);
    final_payload.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(final_payload))
}

pub async fn decrypt_qr_payload(payload: DecryptPayload, state: Arc<ServerState>) -> Result<String, String> {
    use aes_gcm::{Aes256Gcm, Key, Nonce, aead::{Aead, KeyInit}};
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use sha2::{Digest, Sha512};
    use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

    let data = BASE64.decode(&payload.ciphertext_base64).map_err(|_| "Invalid Base64 format")?;
    if data.len() < 32 + 12 + 16 { return Err("Ciphertext too short".to_string()); }

    let mut ephemeral_pub_bytes = [0u8; 32];
    ephemeral_pub_bytes.copy_from_slice(&data[0..32]);
    let ephemeral_pub = X25519PublicKey::from(ephemeral_pub_bytes);

    let nonce_bytes = &data[32..44];
    let nonce = Nonce::from_slice(nonce_bytes);
    let ciphertext = &data[44..];

    let local_ed25519 = state.keypair.clone().try_into_ed25519().map_err(|_| "Local identity is not Ed25519")?;
    let secret_bytes = local_ed25519.secret();

    let mut hasher = Sha512::new();
    hasher.update(secret_bytes);
    let hash = hasher.finalize();

    let mut x25519_sk_bytes = [0u8; 32];
    x25519_sk_bytes.copy_from_slice(&hash[0..32]);
    x25519_sk_bytes[0] &= 248;
    x25519_sk_bytes[31] &= 127;
    x25519_sk_bytes[31] |= 64;

    let local_x25519_secret = StaticSecret::from(x25519_sk_bytes);
    let shared_secret = local_x25519_secret.diffie_hellman(&ephemeral_pub);

    let cipher_key = Key::<Aes256Gcm>::from_slice(shared_secret.as_bytes());
    let cipher = Aes256Gcm::new(cipher_key);

    let plaintext_bytes = cipher.decrypt(nonce, ciphertext).map_err(|_| "Decryption failed")?;
    String::from_utf8(plaintext_bytes).map_err(|_| "Decrypted data is not valid UTF-8".to_string())
}

pub fn toggle_eco_mode(enabled: bool) { ECO_MODE.store(enabled, Ordering::Relaxed); }

pub fn set_mining_state(active: bool) { IS_MINING.store(active, Ordering::Relaxed); }

pub async fn get_state(state: Arc<ServerState>) -> Result<SimUpdate, String> {
    let update = state.last_sim_update.read().await;
    Ok(update.clone().unwrap_or(SimUpdate {
        tick: 0, node_id: state.local_peer_id.clone(), net_entropy: 0, void_events: 0, surplus_events: 0, peer_count: 0,
        local_vault_books: 0, local_prime_value: 0, local_counts: 0, local_nonce: 0, unit_precedents: std::collections::HashMap::new(),
        hash_rate: 0, active_heuristic: None, recent_transfers: vec![], external_addrs: vec![], nat_status: "Unknown".to_string(),
        state_root: "0000000000000000000000000000000000000000000000000000000000000000".to_string(), eco_mode_active: false,
    }))
}

pub async fn do_transfer(to: String, amount: u64, unit: crate::core::Unit, state: &Arc<ServerState>) -> Result<String, String> {
    use std::str::FromStr;

    // Structural Address Validation
    if libp2p::PeerId::from_str(&to).is_err() {
        return Err("Invalid receiver address format.".to_string());
    }
    // Prevent Self Transfers / Ghost Burn
    if to == state.local_peer_id {
        return Err("Cannot transfer value to yourself.".to_string());
    }

    let mut local_state = state.local_state.write().await;
    let refmount = if unit == crate::core::Unit::Twin { 1 } else { crate::core::PRIMES[(unit.counts().saturating_sub(1)) as usize] };
    let amt = refmount * amount;

    if local_state.prime_value < amt {
        return Err("Insufficient value".to_string());
    }

    let receiver_state = state.ledger.get_local_state(&to);
    let target_old_counts = receiver_state.counts;
    let target_new_val = receiver_state.prime_value.saturating_add(amt);
    let target_new_counts_idx = crate::core::get_ordinal_for_prime(target_new_val);
    let target_leap = target_new_counts_idx as i64 + 1 - target_old_counts as i64;

    let source_old_counts = local_state.counts;
    let source_new_val = local_state.prime_value.saturating_sub(amt);
    let source_new_counts_idx = crate::core::get_ordinal_for_prime(source_new_val);
    let source_leap = source_new_counts_idx as i64 + 1 - source_old_counts as i64;

    let net_leap = source_leap + target_leap;

    // Mutate memory immediately to prevent Mempool Revert Bug
    local_state.prime_value -= amt;
    local_state.nonce += 1;
    let nonce = local_state.nonce;

    let mut payload = Vec::new();
    payload.extend_from_slice(CHAIN_ID);
    payload.extend_from_slice(state.local_peer_id.as_bytes());
    payload.extend_from_slice(to.as_bytes());
    payload.extend_from_slice(&amt.to_le_bytes());
    payload.extend_from_slice(&nonce.to_le_bytes());

    let signature = state.keypair.sign(&payload).unwrap();
    let sender_pk = state.keypair.public().encode_protobuf();

    let mut hasher = blake3::Hasher::new();
    hasher.update(b"transfer_event_v1");
    hasher.update(&payload);
    hasher.update(&signature);
    let event_id: [u8; 32] = hasher.finalize().into();

    let transfer_event = Event {
        id: event_id,
        parent_ids: vec![local_state.last_hash],
        event_type: EventType::Transfer { sender: state.local_peer_id.clone(), sender_pk, receiver: to.clone(), amount_prime: amt, heuristic: unit, nonce, signature },
        entropy_delta: net_leap,
        state_root: *state.ledger.state_root.read().unwrap(),
    };

    let tx_bytes = bincode::serialize(&transfer_event).unwrap();
    state.mempool.lock().await.push(tx_bytes);

    Ok("Transfer pooled".to_string())
}

pub fn run() {
    println!("PrimeTime Node Engine running");
}
