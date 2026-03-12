use sled::Tree;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use crate::pow::Proof;
use crate::core::{Unit, get_ordinal_for_prime, PRIMES, CHAIN_ID};
use crate::config::CONFIG;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EventType {
    Mint { proof: Proof, heuristic: Option<Unit> },
    Tick { proof: Proof },
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
    pub vault_books: u64, pub counts: usize, pub prime_value: u64, pub reserved_balance: u64,
    pub balance_adjustment: u64, pub active_heuristic: Option<Unit>, pub active_book_counts: Option<usize>,
    #[serde(with = "crate::utils::serde_hex")] pub last_hash: [u8; 32],
    pub nonce: u64,
}

pub struct Ledger {
    events_db: Tree, transfers_db: Tree, state_db: Tree, meta_db: Tree,
    pub net_entropy: std::sync::atomic::AtomicI64, pub surplus_events: std::sync::atomic::AtomicI64, pub void_events: std::sync::atomic::AtomicI64,
    pub state_root: std::sync::RwLock<[u8; 32]>,
}

impl Ledger {
    pub fn new(path: &str) -> Arc<Self> {
        let db = sled::open(path).expect("Failed to open sled DB");
        let events_db = db.open_tree("events").unwrap();
        let transfers_db = db.open_tree("transfers").unwrap();
        let state_db = db.open_tree("state").unwrap();
        let meta_db = db.open_tree("meta").unwrap();

        let mut entropy = 0; let mut surplus = 0; let mut voids = 0; let mut sr = [0u8; 32];
        if let Ok(Some(ent)) = meta_db.get("net_entropy") { entropy = i64::from_le_bytes(ent.as_ref().try_into().unwrap_or([0; 8])); }
        if let Ok(Some(s)) = meta_db.get("surplus") { surplus = i64::from_le_bytes(s.as_ref().try_into().unwrap_or([0; 8])); }
        if let Ok(Some(v)) = meta_db.get("voids") { voids = i64::from_le_bytes(v.as_ref().try_into().unwrap_or([0; 8])); }
        if let Ok(Some(r)) = meta_db.get("state_root") { if r.len() == 32 { sr.copy_from_slice(&r); } }

        Arc::new(Ledger {
            events_db, transfers_db, state_db, meta_db,
            net_entropy: std::sync::atomic::AtomicI64::new(entropy),
            surplus_events: std::sync::atomic::AtomicI64::new(surplus),
            void_events: std::sync::atomic::AtomicI64::new(voids),
            state_root: std::sync::RwLock::new(sr),
        })
    }

    fn increment_counter(&self, key: &str) -> u64 {
        let mut current = 0;
        if let Ok(Some(val)) = self.meta_db.get(key) {
            if val.len() == 8 { let mut buf = [0u8; 8]; buf.copy_from_slice(&val); current = u64::from_le_bytes(buf); }
        }
        self.meta_db.insert(key, &(current + 1).to_le_bytes()).unwrap();
        current
    }

    pub fn save_event(&self, event: &Event) {
        let serialized = bincode::serialize(event).unwrap();
        self.events_db.insert(&event.id, serialized.clone()).unwrap();

        let mut sr = self.state_root.write().unwrap();
        let mut payload = sr.to_vec();
        payload.extend_from_slice(&event.id);
        *sr = blake3::hash(&payload).into();
        self.meta_db.insert("state_root", &*sr).unwrap();

        let idx = self.increment_counter("event_count");
        self.meta_db.insert(format!("log_{}", idx), &event.id).unwrap();
        let horizon_limit = 5000;
        if idx > horizon_limit {
            let prune_idx = idx - horizon_limit;
            if let Ok(Some(hash_bytes)) = self.meta_db.get(format!("log_{}", prune_idx)) {
                let _ = self.events_db.remove(&hash_bytes);
                let _ = self.meta_db.remove(format!("log_{}", prune_idx));
            }
        }

        if matches!(event.event_type, EventType::Transfer { .. }) {
            let t_idx = self.increment_counter("transfer_idx");
            self.transfers_db.insert(t_idx.to_be_bytes(), serialized).unwrap();
            if t_idx > 100 { let _ = self.transfers_db.remove((t_idx - 100).to_be_bytes()); }
        }

        let old_entropy = self.net_entropy.fetch_add(event.entropy_delta, std::sync::atomic::Ordering::SeqCst);
        let new_entropy = old_entropy + event.entropy_delta;

        let tbc = CONFIG.total_book_counts;
        if new_entropy >= tbc {
            self.net_entropy.fetch_sub(tbc, std::sync::atomic::Ordering::SeqCst);
            self.surplus_events.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        } else if new_entropy <= -tbc {
            self.net_entropy.fetch_add(tbc, std::sync::atomic::Ordering::SeqCst);
            self.void_events.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        self.meta_db.insert("net_entropy", &self.net_entropy.load(std::sync::atomic::Ordering::SeqCst).to_le_bytes()).unwrap();
        self.meta_db.insert("surplus", &self.surplus_events.load(std::sync::atomic::Ordering::SeqCst).to_le_bytes()).unwrap();
        self.meta_db.insert("voids", &self.void_events.load(std::sync::atomic::Ordering::SeqCst).to_le_bytes()).unwrap();
    }

    pub fn verify_and_process_event(&self, event: &Event) -> bool {
        if self.events_db.contains_key(&event.id).unwrap_or(false) { return false; }

        match &event.event_type {
            EventType::Mint { proof, .. } | EventType::Tick { proof } => {
                if proof.iterations > 0 && !crate::pow::verify_proof(proof) { return false; }

                self.save_event(event);

                for (_, tx_bytes) in &proof.injected_txs {
                    if let Ok(tx_event) = bincode::deserialize::<Event>(tx_bytes) {
                        if matches!(tx_event.event_type, EventType::Transfer { .. }) {
                            self.verify_and_process_event(&tx_event);
                        }
                    }
                }
                true
            },
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

                let mut sender_state = self.get_local_state(sender);
                if sender_state.nonce >= *nonce || (sender_state.prime_value.saturating_sub(sender_state.reserved_balance)) < *amount_prime { return false; }

                sender_state.prime_value -= amount_prime;
                sender_state.counts = get_ordinal_for_prime(sender_state.prime_value) + 1;
                let sender_base = if sender_state.counts > 0 { PRIMES[sender_state.counts - 1] } else { 2 };
                sender_state.balance_adjustment = sender_state.prime_value.saturating_sub(sender_base);
                sender_state.nonce = *nonce;
                self.save_local_state(sender, &sender_state);

                let mut receiver_state = self.get_local_state(receiver);
                receiver_state.prime_value += amount_prime;
                receiver_state.counts = get_ordinal_for_prime(receiver_state.prime_value) + 1;
                let receiver_base = PRIMES[receiver_state.counts - 1];
                receiver_state.balance_adjustment = receiver_state.prime_value.saturating_sub(receiver_base);
                self.save_local_state(receiver, &receiver_state);

                self.save_event(event);
                true
            }
        }
    }

    pub fn get_all_events(&self) -> Vec<Event> {
        self.events_db.iter().filter_map(|res| res.ok()).filter_map(|(_, val)| bincode::deserialize::<Event>(&val).ok()).collect()
    }

    pub fn get_recent_transfers(&self, limit: usize) -> Vec<Event> {
        self.transfers_db.iter().rev().take(limit).filter_map(|res| res.ok()).filter_map(|(_, val)| bincode::deserialize::<Event>(&val).ok()).collect()
    }

    pub fn get_local_state(&self, id: &str) -> LocalState {
        if let Ok(Some(val)) = self.state_db.get(id) { bincode::deserialize(&val).unwrap_or_else(|_| Self::default_state()) } else { Self::default_state() }
    }

    pub fn save_local_state(&self, id: &str, state: &LocalState) {
        let serialized = bincode::serialize(state).unwrap();
        self.state_db.insert(id, serialized).unwrap();
    }

    fn default_state() -> LocalState {
        LocalState {
            vault_books: 2, counts: 1, prime_value: 2, reserved_balance: 0, balance_adjustment: 0,
            active_heuristic: None, active_book_counts: None,
            last_hash: [0u8; 32], nonce: 0,
        }
    }
}

impl LocalState {
    pub fn update_prime_value(&mut self) {
        let mut ordinal_idx = if self.counts > 0 { self.counts - 1 } else { 0 };
        ordinal_idx = ordinal_idx.min(PRIMES.len() - 1);
        self.prime_value = PRIMES[ordinal_idx] + self.balance_adjustment;
    }
}
