use serde::{Serialize, Deserialize};
use std::sync::Arc;
use crate::ledger::{Event, Ledger};
use crate::core::Unit;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SimUpdate {
    pub tick: u64, pub node_id: String, pub net_entropy: i64, pub void_events: i64, pub surplus_events: i64,
    pub peer_count: usize, pub local_vault_books: u64, pub local_prime_value: u64, pub local_counts: usize,
    pub local_nonce: u64, pub unit_precedents: std::collections::HashMap<String, u64>, pub hash_rate: u64,
    pub active_heuristic: Option<Unit>, pub recent_transfers: Vec<Event>,
    pub external_addrs: Vec<String>, pub nat_status: String,
    pub state_root: String,
}

#[derive(Serialize, Deserialize)]
pub struct TransferRequest { pub to: String, pub amount: u64, pub unit: Unit, }

pub struct ServerState {
    pub ledger: Arc<Ledger>, pub event_tx: tokio::sync::mpsc::Sender<Event>,
    pub local_peer_id: String, pub keypair: libp2p::identity::Keypair, pub local_state: Arc<tokio::sync::RwLock<crate::ledger::LocalState>>,
    pub last_sim_update: Arc<tokio::sync::RwLock<Option<SimUpdate>>>,
    pub mempool: Arc<tokio::sync::Mutex<Vec<Vec<u8>>>>,
}
