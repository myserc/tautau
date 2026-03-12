#![allow(clippy::type_complexity)]

pub mod utils {
    pub mod serde_hex {
        use serde::{Deserialize, Deserializer, Serializer};
        pub fn serialize<S: Serializer>(data: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(&hex::encode(data))
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
            let s = String::deserialize(d)?;
            let mut arr = [0u8; 32];
            if let Ok(decoded) = hex::decode(s) {
                if decoded.len() == 32 { arr.copy_from_slice(&decoded); }
            }
            Ok(arr)
        }
    }
    pub mod serde_hex_vec {
        use serde::{Deserialize, Deserializer, Serializer, ser::SerializeSeq};
        pub fn serialize<S: Serializer>(data: &Vec<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
            let mut seq = s.serialize_seq(Some(data.len()))?;
            for e in data { seq.serialize_element(&hex::encode(e))?; }
            seq.end()
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<[u8; 32]>, D::Error> {
            let vec_s = Vec::<String>::deserialize(d)?;
            let mut res = Vec::new();
            for s in vec_s {
                let mut arr = [0u8; 32];
                if let Ok(decoded) = hex::decode(s) {
                    if decoded.len() == 32 { arr.copy_from_slice(&decoded); res.push(arr); }
                }
            }
            Ok(res)
        }
    }
}

pub mod config {
    use lazy_static::lazy_static;
    use crate::core::Unit;

    pub struct Config {
        pub mode: String,
        pub limit: usize,
        pub total_book_counts: i64,
        pub standard_mint_scarcity: u64,
        pub units: Vec<Unit>,
        pub num_agents: u32,
    }

    lazy_static! {
        pub static ref CONFIG: Config = {
            let args: Vec<String> = std::env::args().collect();
            let mode = args.iter().find(|&a| a.to_lowercase() == "coop").map(|_| "coop").unwrap_or("finn").to_string();
            
            if mode == "coop" {
                println!("⚙️ Booting Physics Profile: COOP (1,000,000 Nodes)");
                Config {
                    mode: "coop".to_string(), limit: 20_000_000, total_book_counts: 648_000, standard_mint_scarcity: 9_731_081,
                    units: vec![Unit::Quadrant, Unit::Day, Unit::Degree, Unit::Minute, Unit::Twin], num_agents: 1_000_000,
                }
            } else {
                println!("⚙️ Booting Physics Profile: FINN (250,000 Nodes)");
                Config {
                    mode: "finn".to_string(), limit: 1_200_000, total_book_counts: 10_800, standard_mint_scarcity: 114_113,
                    units: vec![Unit::Day, Unit::Degree, Unit::Twin], num_agents: 250_000,
                }
            }
        };
    }
}

pub mod core {
    use lazy_static::lazy_static;
    use std::collections::HashMap;
    use crate::config::CONFIG;

    pub const CHAIN_ID: &[u8] = b"primetime-mainnet-v2";

    lazy_static! { pub static ref PRIMES: Vec<u64> = generate_primes(CONFIG.limit); }

    pub fn generate_primes(limit: usize) -> Vec<u64> {
        let mut sieve = vec![true; limit];
        let mut primes = Vec::with_capacity(limit / 10);
        for p in 2..limit {
            if sieve[p] {
                primes.push(p as u64);
                let mut i = p * p;
                while i < limit { sieve[i] = false; i += p; }
            }
        }
        primes
    }

    pub fn get_ordinal_for_prime(value: u64) -> usize {
        let idx = PRIMES.partition_point(|&x| x <= value);
        if idx == 0 { 0 } else { idx - 1 }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub enum Unit { Quadrant, Day, Degree, Minute, Twin }

    impl Unit {
        pub fn counts(&self) -> usize {
            if CONFIG.mode == "finn" {
                match self { Unit::Quadrant => 0, Unit::Day => 1_800, Unit::Degree => 30, Unit::Minute => 0, Unit::Twin => 1 }
            } else {
                match self { Unit::Quadrant => 162_000, Unit::Day => 43_200, Unit::Degree => 1_800, Unit::Minute => 30, Unit::Twin => 1 }
            }
        }
        pub fn all() -> Vec<Unit> { CONFIG.units.clone() }
    }

    pub struct HeuristicStandard { pub mint_scarcity: u64, pub mint_counts: usize, pub precedent: u64 }

    lazy_static! {
        pub static ref STANDARDS: HashMap<Unit, HeuristicStandard> = {
            let mut map = HashMap::new();
            let _ = &*PRIMES; 
            for unit in Unit::all() {
                let precedent = PRIMES[unit.counts() - 1];
                let mint_scarcity = (CONFIG.standard_mint_scarcity / precedent) * precedent;
                let mint_counts = get_ordinal_for_prime(mint_scarcity) + 1;
                map.insert(unit, HeuristicStandard { mint_scarcity, mint_counts, precedent });
            }
            map
        };
    }
}

pub mod pow {
    use serde::{Serialize, Deserialize};

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct Proof {
        #[serde(with = "crate::utils::serde_hex")] pub start_hash: [u8; 32],
        #[serde(with = "crate::utils::serde_hex")] pub end_hash: [u8; 32],
        pub iterations: usize,
        pub injected_txs: Vec<(usize, Vec<u8>)>, // (iteration_index, serialized_transfer_event)
    }

    pub fn mine_iterations(start_hash: &[u8; 32], target_iterations: usize, pending_events: Vec<Vec<u8>>) -> Proof {
        let mut current_bytes = *start_hash;
        
        let mut injected_txs = Vec::new();
        let num_events = pending_events.len();
        let events_per_iteration = if num_events == 0 { 0 } else { (target_iterations / num_events).max(1) };
        let mut event_idx = 0;
        
        for i in 0..target_iterations {
            current_bytes = blake3::hash(&current_bytes).into();
            
            while event_idx < num_events && (i % events_per_iteration == 0 || i == target_iterations - 1) {
                let tx_bytes = &pending_events[event_idx];
                let mut payload = current_bytes.to_vec();
                payload.extend_from_slice(tx_bytes);
                current_bytes = blake3::hash(&payload).into();
                injected_txs.push((i, tx_bytes.clone()));
                event_idx += 1;
                break; 
            }
            
            if i == target_iterations - 1 {
                while event_idx < num_events {
                    let tx_bytes = &pending_events[event_idx];
                    let mut payload = current_bytes.to_vec();
                    payload.extend_from_slice(tx_bytes);
                    current_bytes = blake3::hash(&payload).into();
                    injected_txs.push((i, tx_bytes.clone()));
                    event_idx += 1;
                }
            }
        }
        
        Proof { start_hash: *start_hash, end_hash: current_bytes, iterations: target_iterations, injected_txs }
    }

    pub fn verify_proof(proof: &Proof) -> bool {
        let mut current_bytes = proof.start_hash;
        let mut inject_idx = 0;
        for i in 0..proof.iterations {
            current_bytes = blake3::hash(&current_bytes).into();
            while inject_idx < proof.injected_txs.len() && proof.injected_txs[inject_idx].0 == i {
                let mut payload = current_bytes.to_vec();
                payload.extend_from_slice(&proof.injected_txs[inject_idx].1);
                current_bytes = blake3::hash(&payload).into();
                inject_idx += 1;
            }
        }
        current_bytes == proof.end_hash
    }
}

pub mod ledger {
    use sled::{Db, Tree};
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
            
            // Update Rolling State Root
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
}

pub mod p2p {
    use libp2p::{ gossipsub, kad, noise, swarm::NetworkBehaviour, tcp, yamux, PeerId, Swarm, request_response, mdns, autonat, relay, dcutr, identify };
    use libp2p::kad::store::MemoryStore;
    use libp2p::identity::Keypair;
    use serde::{Deserialize, Serialize};
    use std::time::Duration;
    use crate::core::Unit;
    use crate::ledger::Event;

    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "P2PBehaviourEvent")]
    pub struct P2PBehaviour {
        pub gossipsub: gossipsub::Behaviour, pub kademlia: kad::Behaviour<MemoryStore>, pub sync_req_resp: request_response::cbor::Behaviour<SyncRequest, SyncResponse>,
        pub mdns: mdns::tokio::Behaviour, pub autonat: autonat::Behaviour, pub relay_client: relay::client::Behaviour, pub dcutr: dcutr::Behaviour, pub identify: identify::Behaviour,
    }

    #[derive(Debug)]
    pub enum P2PBehaviourEvent {
        Gossipsub(gossipsub::Event), Kademlia(kad::Event), SyncReqResp(request_response::Event<SyncRequest, SyncResponse>),
        Mdns(mdns::Event), Autonat(autonat::Event), RelayClient(relay::client::Event), Dcutr(dcutr::Event), Identify(identify::Event),
    }

    impl From<gossipsub::Event> for P2PBehaviourEvent { fn from(e: gossipsub::Event) -> Self { Self::Gossipsub(e) } }
    impl From<kad::Event> for P2PBehaviourEvent { fn from(e: kad::Event) -> Self { Self::Kademlia(e) } }
    impl From<request_response::Event<SyncRequest, SyncResponse>> for P2PBehaviourEvent { fn from(e: request_response::Event<SyncRequest, SyncResponse>) -> Self { Self::SyncReqResp(e) } }
    impl From<mdns::Event> for P2PBehaviourEvent { fn from(e: mdns::Event) -> Self { Self::Mdns(e) } }
    impl From<autonat::Event> for P2PBehaviourEvent { fn from(e: autonat::Event) -> Self { Self::Autonat(e) } }
    impl From<relay::client::Event> for P2PBehaviourEvent { fn from(e: relay::client::Event) -> Self { Self::RelayClient(e) } }
    impl From<dcutr::Event> for P2PBehaviourEvent { fn from(e: dcutr::Event) -> Self { Self::Dcutr(e) } }
    impl From<identify::Event> for P2PBehaviourEvent { fn from(e: identify::Event) -> Self { Self::Identify(e) } }

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct SyncRequest { pub from_tick: u64, }
    #[derive(Debug, Serialize, Deserialize)]
    pub struct SyncResponse { pub events: Vec<Event>, }

    pub struct P2PNode { pub swarm: Swarm<P2PBehaviour>, pub peer_id: PeerId, pub keypair: Keypair, }

    impl P2PNode {
        pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let local_key = Keypair::generate_ed25519();
            let local_peer_id = PeerId::from(local_key.public());
            
            let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
                .with_tokio()
                .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
                .with_relay_client(noise::Config::new, yamux::Config::default)?
                .with_behaviour(|key, relay_client| {
                    let gossip_config = gossipsub::ConfigBuilder::default().heartbeat_interval(Duration::from_secs(1)).validation_mode(gossipsub::ValidationMode::Strict).build().unwrap();
                    let mut gossipsub = gossipsub::Behaviour::new(gossipsub::MessageAuthenticity::Signed(key.clone()), gossip_config).unwrap();
                    gossipsub.subscribe(&gossipsub::IdentTopic::new("primetime-dag")).unwrap();

                    let store = MemoryStore::new(local_peer_id);
                    let kademlia = kad::Behaviour::new(local_peer_id, store);
                    let sync_req_resp = request_response::cbor::Behaviour::new([(libp2p::StreamProtocol::new("/primetime/sync/1.0.0"), request_response::ProtocolSupport::Full)], request_response::Config::default());
                    let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id).unwrap();
                    let autonat = autonat::Behaviour::new(local_peer_id, autonat::Config::default());
                    let dcutr = dcutr::Behaviour::new(local_peer_id);
                    let identify = identify::Behaviour::new(identify::Config::new("/primetime/2.0.0".into(), key.public()));

                    P2PBehaviour { gossipsub, kademlia, sync_req_resp, mdns, autonat, relay_client, dcutr, identify }
                })?
                .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
                .build();
                
            Ok(P2PNode { swarm, peer_id: local_peer_id, keypair: local_key })
        }
    }
}

pub mod server {
    use axum::{
        extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State as AxumState, Json},
        response::{Html, IntoResponse}, routing::{get, post}, Router,
    };
    use tokio::sync::{broadcast, mpsc};
    use serde::{Serialize, Deserialize};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use crate::ledger::{Event, EventType, Ledger};
    use crate::core::{Unit, CHAIN_ID};
    use crate::config::CONFIG;

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct SimUpdate {
        pub tick: u64, pub node_id: String, pub net_entropy: i64, pub void_events: i64, pub surplus_events: i64,
        pub peer_count: usize, pub local_vault_books: u64, pub local_prime_value: u64, pub local_counts: usize,
        pub local_nonce: u64, pub unit_precedents: std::collections::HashMap<String, u64>, pub hash_rate: u64, 
        pub active_heuristic: Option<crate::core::Unit>, pub recent_transfers: Vec<crate::ledger::Event>,
        pub external_addrs: Vec<String>, pub nat_status: String,
        pub state_root: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct TransferRequest { pub to: String, pub amount: u64, pub unit: Unit, }

    pub struct ServerState {
        pub tx: broadcast::Sender<String>, pub ledger: Arc<Ledger>, pub event_tx: mpsc::Sender<Event>,
        pub local_peer_id: String, pub keypair: libp2p::identity::Keypair, pub local_state: Arc<tokio::sync::RwLock<crate::ledger::LocalState>>,
        pub last_sim_update: Arc<tokio::sync::RwLock<Option<SimUpdate>>>,
        pub mempool: Arc<tokio::sync::Mutex<Vec<Vec<u8>>>>,
    }

    pub async fn start_server(state: Arc<ServerState>, port: u16) {
        let app = Router::new().route("/", get(serve_dashboard)).route("/ws", get(ws_handler)).route("/api/state", get(get_state)).route("/api/transfer", post(do_transfer)).with_state(state);
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        println!("🚀 Starting UI Server on http://{}", addr);
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    }

    async fn get_state(AxumState(state): AxumState<Arc<ServerState>>) -> Json<SimUpdate> {
        let update = state.last_sim_update.read().await;
        Json(update.clone().unwrap_or(SimUpdate {
            tick: 0, node_id: state.local_peer_id.clone(), net_entropy: 0, void_events: 0, surplus_events: 0, peer_count: 0,
            local_vault_books: 0, local_prime_value: 0, local_counts: 0, local_nonce: 0, unit_precedents: std::collections::HashMap::new(),
            hash_rate: 0, active_heuristic: None, recent_transfers: vec![], external_addrs: vec![], nat_status: "Unknown".to_string(),
            state_root: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        }))
    }

    async fn do_transfer(AxumState(state): AxumState<Arc<ServerState>>, Json(req): Json<TransferRequest>) -> impl IntoResponse {
        let mut local_state = state.local_state.write().await;
        if local_state.prime_value < req.amount { return (axum::http::StatusCode::BAD_REQUEST, "Insufficient funds").into_response(); }

        let receiver_state = state.ledger.get_local_state(&req.to);
        let target_old_counts = receiver_state.counts;
        let target_new_val = receiver_state.prime_value.saturating_add(req.amount);
        let target_new_counts_idx = crate::core::get_ordinal_for_prime(target_new_val);
        let target_leap = target_new_counts_idx as i64 + 1 - target_old_counts as i64;
        
        let source_old_counts = local_state.counts;
        let source_new_val = local_state.prime_value.saturating_sub(req.amount);
        let source_new_counts_idx = crate::core::get_ordinal_for_prime(source_new_val);
        let source_leap = source_new_counts_idx as i64 + 1 - source_old_counts as i64;
        
        let net_leap = source_leap + target_leap;

        local_state.nonce += 1;
        let nonce = local_state.nonce;
        
        let mut payload = Vec::new();
        payload.extend_from_slice(CHAIN_ID);
        payload.extend_from_slice(state.local_peer_id.as_bytes());
        payload.extend_from_slice(req.to.as_bytes());
        payload.extend_from_slice(&req.amount.to_le_bytes());
        payload.extend_from_slice(&nonce.to_le_bytes());
        
        let signature = state.keypair.sign(&payload).unwrap();
        let sender_pk = state.keypair.public().encode_protobuf();
        let event_id: [u8; 32] = rand::random();
        
        let transfer_event = Event {
            id: event_id, parent_ids: vec![local_state.last_hash],
            event_type: EventType::Transfer { sender: state.local_peer_id.clone(), sender_pk, receiver: req.to.clone(), amount_prime: req.amount, heuristic: req.unit, nonce, signature },
            entropy_delta: net_leap,
            state_root: *state.ledger.state_root.read().unwrap(),
        };

        let tx_bytes = bincode::serialize(&transfer_event).unwrap();
        state.mempool.lock().await.push(tx_bytes);

        (axum::http::StatusCode::OK, "Transfer pooled for local PoH Injection").into_response()
    }

    async fn ws_handler(ws: WebSocketUpgrade, AxumState(state): AxumState<Arc<ServerState>>) -> impl IntoResponse {
        let tx = state.tx.clone();
        ws.on_upgrade(move |socket| async move {
            let mut rx = tx.subscribe();
            let mut s = socket;
            while let Ok(msg) = rx.recv().await { if s.send(Message::Text(msg)).await.is_err() { break; } }
        })
    }

    async fn serve_dashboard() -> Html<String> {
        let tbc = CONFIG.total_book_counts;
        let sms = CONFIG.standard_mint_scarcity;
        let p_name = CONFIG.mode.to_uppercase();

        let html = format!(r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Prime-Time | Node Operator</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
        @import url('https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700&family=Inter:wght@400;600;800&display=swap');
        :root {{ --bg-dark: #020617; --panel-bg: rgba(15, 23, 42, 0.6); }}
        body {{ font-family: 'Inter', sans-serif; background-color: var(--bg-dark); color: #e2e8f0; background-image: radial-gradient(circle at 50% 0%, rgba(34, 211, 238, 0.1) 0%, transparent 50%), linear-gradient(rgba(255, 255, 255, 0.03) 1px, transparent 1px), linear-gradient(90deg, rgba(255, 255, 255, 0.03) 1px, transparent 1px); background-size: 100% 100%, 40px 40px, 40px 40px; overflow-x: hidden; }}
        .font-mono {{ font-family: 'JetBrains Mono', monospace; }}
        .glass-panel {{ background: var(--panel-bg); backdrop-filter: blur(12px); border: 1px solid rgba(255, 255, 255, 0.08); box-shadow: 0 4px 30px rgba(0, 0, 0, 0.1); }}
        @keyframes pulse-glow {{ 0%, 100% {{ box-shadow: 0 0 15px rgba(34, 211, 238, 0.1); border-color: rgba(34, 211, 238, 0.3); }} 50% {{ box-shadow: 0 0 25px rgba(34, 211, 238, 0.2); border-color: rgba(34, 211, 238, 0.6); }} }}
        .active-standard {{ animation: pulse-glow 3s infinite; }}
        ::-webkit-scrollbar {{ width: 6px; height: 6px; }}
        ::-webkit-scrollbar-track {{ background: rgba(15, 23, 42, 0.5); }}
        ::-webkit-scrollbar-thumb {{ background: #334155; border-radius: 3px; }}
    </style>
</head>
<body class="p-4 md:p-8 min-h-screen pb-32">
    <div class="max-w-[1400px] mx-auto space-y-8">
        <header class="flex justify-between items-center border-b border-slate-800 pb-6">
            <div>
                <h1 class="text-2xl font-black tracking-tighter text-white">PRIME-TIME <span class="text-xs bg-cyan-500/10 text-cyan-400 px-2 py-1 rounded border border-cyan-500/20 font-mono">NODE OPERATOR</span></h1>
                <p class="text-[10px] text-slate-500 tracking-widest uppercase mt-1 font-bold">Byte-Pure PoH Engine v8.0 | PROFILE: {p_name}</p>
                <p class="text-[8px] text-slate-600 font-mono mt-1">STATE ROOT: <span id="state_root_display">0000...</span></p>
            </div>
            <div class="flex gap-4 text-right font-mono">
                <div><p class="text-[10px] text-slate-500 uppercase">System Tick</p><p class="text-sm font-bold text-white" id="tick">0</p></div>
                <div><p class="text-[10px] text-slate-500 uppercase">Peers</p><p class="text-sm font-bold text-cyan-400" id="peer_count">0</p></div>
            </div>
        </header>

        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-4">
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-rose-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">Voids Detected</p><p class="text-xl font-bold text-white font-mono" id="voids">0</p>
            </div>
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-emerald-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">Surplus Mints</p><p class="text-xl font-bold text-white font-mono" id="surplus">0</p>
            </div>
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-cyan-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">Net Entropy</p><p class="text-xl font-bold text-white font-mono" id="entropy_val">0</p>
            </div>
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-yellow-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">PoTH Hash Rate</p><p class="text-xl font-bold text-white font-mono"><span id="hash_rate">0</span> <span class="text-[10px] text-yellow-500">H/S</span></p>
            </div>
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-purple-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">NAT / Reachability</p><p class="text-sm font-bold text-cyan-400 font-mono truncate" id="nat_status">Detecting...</p>
            </div>
        </div>

        <div class="grid grid-cols-1 xl:grid-cols-3 gap-8">
            <div class="xl:col-span-1 space-y-4">
                <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest">Local Node Agent</h3>
                <div id="agent-card" class="glass-panel p-6 rounded-2xl relative overflow-hidden active-standard border-slate-700">
                    <div class="absolute top-0 left-0 h-1 bg-cyan-400 transition-all duration-300" id="progress-bar" style="width: 0%"></div>
                    <div class="flex justify-between items-start mb-6">
                        <div>
                            <div class="flex items-center gap-2">
                                <h4 class="text-lg font-black text-white tracking-tight">ALPHA_NODE</h4>
                                <span class="text-[10px] bg-cyan-500/10 text-cyan-400 px-1.5 py-0.5 rounded border border-cyan-500/20 font-mono">ED25519</span>
                            </div>
                            <div id="active-h-display" class="text-[10px] font-bold mt-1 text-purple-400 uppercase"></div>
                            <div class="flex items-center gap-2 mt-1"><span class="text-[8px] px-1.5 py-0.5 rounded bg-emerald-500/10 text-emerald-400 font-bold uppercase" id="status-badge">MINING PoTH</span></div>
                        </div>
                        <div class="text-right"><p class="text-[8px] text-slate-500 uppercase font-bold">Nonce</p><p class="text-xs font-bold text-white font-mono" id="local_nonce">0</p></div>
                    </div>
                    <div class="space-y-4">
                        <div class="bg-slate-900/50 p-4 rounded-xl border border-slate-800">
                            <p class="text-[10px] text-slate-500 uppercase font-bold mb-1">Scarcity Index (Prime Value)</p>
                            <p class="text-2xl font-black text-white font-mono tracking-tighter" id="vault_books_val">0</p>
                        </div>
                        <div class="bg-slate-900/50 p-4 rounded-xl border border-slate-800">
                            <p class="text-[10px] text-slate-500 uppercase font-bold mb-2">Vault Balance</p>
                            <div class="flex items-baseline gap-2"><p class="text-xl font-bold text-emerald-400 font-mono" id="vault_books_count">0</p><span class="text-xs text-slate-500 font-bold uppercase">Books</span></div>
                        </div>
                    </div>
                </div>

                <div class="glass-panel p-6 rounded-2xl border-slate-700 mt-4">
                    <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest mb-4">Manual Transfer Terminal</h3>
                    <div class="space-y-3">
                        <div>
                            <label class="text-[10px] text-slate-500 uppercase font-bold">Target Peer ID</label>
                            <input type="text" id="tx_to" class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500" placeholder="e.g. 12D3KooW...">
                        </div>
                        <div class="grid grid-cols-2 gap-3">
                            <div>
                                <label class="text-[10px] text-slate-500 uppercase font-bold">PV Amount</label>
                                <input type="number" id="tx_amount" class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500" placeholder="e.g. 5000">
                            </div>
                            <div>
                                <label class="text-[10px] text-slate-500 uppercase font-bold">Heuristic</label>
                                <select id="tx_unit" class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500"></select>
                            </div>
                        </div>
                        <button onclick="initiateTransfer()" class="w-full bg-cyan-500/20 hover:bg-cyan-500/40 border border-cyan-500/50 text-cyan-400 font-bold uppercase text-xs py-2 rounded transition-colors">Queue into PoH Mempool</button>
                        <div id="tx_status" class="text-[10px] font-mono mt-2 text-center h-4"></div>
                    </div>
                </div>

                <div class="space-y-4 pt-4">
                    <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest">PoH Verified Transfers</h3>
                    <div id="transfer-log" class="glass-panel rounded-xl overflow-hidden divide-y divide-slate-800"></div>
                </div>
            </div>

            <div class="xl:col-span-2 space-y-4">
                <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest">Global Entropy Analytics</h3>
                <div class="glass-panel p-6 rounded-2xl h-[400px]"><canvas id="entropyChart"></canvas></div>
            </div>
        </div>
    </div>

    <footer class="fixed bottom-6 left-1/2 -translate-x-1/2 glass-panel px-8 py-4 rounded-[18px] z-50 border-cyan-500/30 shadow-[0_0_30px_rgba(0,0,0,0.5)] min-w-[320px]">
        <div class="flex justify-between items-center gap-12">
            <div class="text-center">
                <p class="text-[9px] text-slate-500 uppercase font-black tracking-tighter mb-1">Days</p>
                <p class="text-2xl font-black text-white font-mono leading-none" id="clock-days">00</p>
            </div>
            <div class="h-8 w-px bg-slate-800"></div>
            <div class="text-center">
                <p class="text-[9px] text-slate-500 uppercase font-black tracking-tighter mb-1">Degrees</p>
                <p class="text-2xl font-black text-cyan-400 font-mono leading-none" id="clock-degrees">00</p>
            </div>
            <div class="h-8 w-px bg-slate-800"></div>
            <div class="text-center">
                <p class="text-[9px] text-slate-500 uppercase font-black tracking-tighter mb-1">Twins</p>
                <p class="text-2xl font-black text-emerald-400 font-mono leading-none" id="clock-twins">00</p>
            </div>
        </div>
    </footer>

    <script>
        const ctx = document.getElementById('entropyChart').getContext('2d');
        const TOTAL_BOOK_COUNTS = {tbc};
        const STANDARD_MINT_SCARCITY = {sms};

        const chart = new Chart(ctx, {{
            type: 'line',
            data: {{ labels: [], datasets: [
                {{ label: 'Net Entropy', borderColor: '#22d3ee', backgroundColor: 'rgba(34, 211, 238, 0.1)', fill: true, data: [], tension: 0.4, borderWeight: 3 }},
                {{ label: '+ Limit', borderColor: '#34d399', borderDash: [5, 5], data: [], pointRadius: 0, borderWidth: 1 }},
                {{ label: '- Limit', borderColor: '#fb7185', borderDash: [5, 5], data: [], pointRadius: 0, borderWidth: 1 }}
            ]}},
            options: {{ responsive: true, maintainAspectRatio: false, animation: false, plugins: {{ legend: {{ display: false }} }}, scales: {{ x: {{ display: false }}, y: {{ grid: {{ color: 'rgba(255,255,255,0.05)' }}, ticks: {{ color: '#64748b', font: {{ family: 'JetBrains Mono', size: 10 }} }} }} }} }}
        }});

        async function initiateTransfer() {{
            const to = document.getElementById('tx_to').value;
            const amount = parseInt(document.getElementById('tx_amount').value);
            const unit = document.getElementById('tx_unit').value;
            const status = document.getElementById('tx_status');
            
            if(!to || !amount) {{ status.innerHTML = '<span class="text-rose-400">Error: Missing fields</span>'; return; }}
            status.innerHTML = '<span class="text-yellow-400">Queuing in Mempool...</span>';
            
            try {{
                const res = await fetch('/api/transfer', {{ method: 'POST', headers: {{'Content-Type': 'application/json'}}, body: JSON.stringify({{to, amount, unit}}) }});
                if(res.ok) {{ status.innerHTML = '<span class="text-emerald-400">Queued for PoH Block!</span>'; document.getElementById('tx_amount').value = ''; }} 
                else {{ status.innerHTML = `<span class="text-rose-400">Failed: ${{await res.text()}}</span>`; }}
            }} catch(e) {{ status.innerHTML = `<span class="text-rose-400">Error: ${{e.message}}</span>`; }}
        }}

        const ws = new WebSocket("ws://" + window.location.host + "/ws");
        ws.onmessage = function(event) {{
            const data = JSON.parse(event.data);
            
            document.getElementById('tick').innerText = data.tick.toLocaleString();
            document.getElementById('peer_count').innerText = data.peer_count;
            document.getElementById('hash_rate').innerText = data.hash_rate.toLocaleString();
            document.getElementById('voids').innerText = data.void_events.toLocaleString();
            document.getElementById('surplus').innerText = data.surplus_events.toLocaleString();
            document.getElementById('entropy_val').innerText = data.net_entropy.toLocaleString();
            document.getElementById('nat_status').innerText = data.nat_status;
            document.getElementById('vault_books_count').innerText = data.local_vault_books.toLocaleString();
            document.getElementById('vault_books_val').innerText = data.local_prime_value.toLocaleString();
            document.getElementById('local_nonce').innerText = data.local_nonce;
            document.getElementById('state_root_display').innerText = data.state_root.slice(0, 16) + '...';

            const tx_unit = document.getElementById('tx_unit');
            if (tx_unit.options.length === 0) {{ tx_unit.innerHTML = Object.keys(data.unit_precedents).map(u => `<option value="${{u}}">${{u}}</option>`).join(''); }}
            
            if (data.active_heuristic) document.getElementById('active-h-display').innerText = 'HEURISTIC: ' + data.active_heuristic;
            else document.getElementById('active-h-display').innerText = '';

            const pct = Math.min(100, (data.local_prime_value / STANDARD_MINT_SCARCITY) * 100);
            document.getElementById('progress-bar').style.width = pct + '%';
            
            const statusBadge = document.getElementById('status-badge');
            if (data.hash_rate > 0) {{
                statusBadge.innerText = 'MINING PoTH';
                statusBadge.className = 'text-[8px] px-1.5 py-0.5 rounded bg-emerald-500/10 text-emerald-400 font-bold uppercase';
                document.getElementById('agent-card').classList.add('active-standard');
            }} else {{
                statusBadge.innerText = 'IDLE';
                statusBadge.className = 'text-[8px] px-1.5 py-0.5 rounded bg-slate-500/10 text-slate-500 font-bold uppercase';
                document.getElementById('agent-card').classList.remove('active-standard');
            }}

            const log = document.getElementById('transfer-log');
            log.innerHTML = data.recent_transfers.map(e => {{
                if(!e.event_type.Transfer) return '';
                const t = e.event_type.Transfer;
                return `
                    <div class="p-3 text-[10px] font-mono flex justify-between items-center bg-slate-900/30">
                        <div class="space-y-1">
                            <div><span class="text-slate-500">From:</span> <span class="text-white">${{t.sender.slice(0, 8)}}...</span></div>
                            <div><span class="text-slate-500">To:</span> <span class="text-white">${{t.receiver.slice(0, 8)}}...</span></div>
                        </div>
                        <div class="text-right">
                            <p class="text-cyan-400 font-bold">${{t.heuristic}}</p>
                            <p class="${{e.entropy_delta > 0 ? 'text-emerald-400' : 'text-rose-400'}}">ΔS: ${{e.entropy_delta > 0 ? '+' : ''}}${{e.entropy_delta}}</p>
                        </div>
                    </div>
                `;
            }}).join('');

            const pv = data.local_prime_value;
            const dayVal = data.unit_precedents["Day"] || 1;
            const degVal = data.unit_precedents["Degree"] || 1;
            const twinVal = data.unit_precedents["Twin"] || 1;

            const days = Math.floor(pv / dayVal); let rem = pv % dayVal;
            const degrees = Math.floor(rem / degVal); rem = rem % degVal;
            const twins = Math.floor(rem / twinVal);

            document.getElementById('clock-days').innerText = days.toString().padStart(2, '0');
            document.getElementById('clock-degrees').innerText = degrees.toString().padStart(2, '0');
            document.getElementById('clock-twins').innerText = twins.toString().padStart(2, '0');

            chart.data.labels.push(data.tick);
            chart.data.datasets[0].data.push(data.net_entropy);
            chart.data.datasets[1].data.push(TOTAL_BOOK_COUNTS);
            chart.data.datasets[2].data.push(-TOTAL_BOOK_COUNTS);
            if(chart.data.labels.length > 50) {{ chart.data.labels.shift(); chart.data.datasets.forEach(ds => ds.data.shift()); }}
            chart.update();
        }};
    </script>
</body>
</html>
        "#);
        Html(html)
    }
}

use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::{interval, Duration, Instant};
use libp2p::swarm::SwarmEvent;
use libp2p::{gossipsub, request_response, mdns, identify, autonat, relay, dcutr};
use libp2p::futures::StreamExt;
use libp2p::Multiaddr;
use clap::{Parser, Subcommand};

use crate::ledger::{Ledger, Event, EventType};
use crate::p2p::{P2PNode, P2PBehaviourEvent, SyncRequest, SyncResponse};
use crate::pow::mine_iterations;
use crate::server::{SimUpdate, start_server, ServerState};
use crate::config::CONFIG;

#[derive(Parser)]
#[command(name = "primetime-node")]
#[command(about = "Prime-Time Node: Arithmodynamic Blockchain Network", long_about = None)]
struct Cli {
    #[command(subcommand)] command: Option<Commands>,
    #[arg(short, long, default_value_t = 3001)] port: u16,
    #[arg(short, long, default_value_t = 0)] swarm_port: u16,
    #[arg(short, long)] bootstrap: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    Finn { #[arg(short, long, default_value_t = 3001)] port: u16, #[arg(short, long, default_value_t = 0)] swarm_port: u16, #[arg(short, long)] bootstrap: Option<String>, },
    Coop { #[arg(short, long, default_value_t = 3001)] port: u16, #[arg(short, long, default_value_t = 0)] swarm_port: u16, #[arg(short, long)] bootstrap: Option<String>, },
    Run { #[arg(short, long, default_value_t = 3001)] port: u16, #[arg(short, long, default_value_t = 0)] swarm_port: u16, #[arg(short, long)] bootstrap: Option<String>, },
    Transfer { #[arg(short, long)] to: String, #[arg(short, long)] amount: u64, #[arg(short, long, default_value = "Day")] unit: String, #[arg(short, long, default_value_t = 3001)] port: u16, },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = &*CONFIG; 
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Transfer { to, amount, unit, port }) => {
            let unit_enum = match unit.as_str() {
                "Day" => crate::core::Unit::Day, "Degree" => crate::core::Unit::Degree, "Twin" => crate::core::Unit::Twin,
                "Quadrant" => crate::core::Unit::Quadrant, "Minute" => crate::core::Unit::Minute,
                _ => return Err("Invalid unit.".into()),
            };
            let client = reqwest::Client::new();
            let resp = client.post(format!("http://localhost:{}/api/transfer", port)).json(&crate::server::TransferRequest { to, amount, unit: unit_enum }).send().await?;
            println!("{}", if resp.status().is_success() { "✨ Queued in Mempool!" } else { "❌ Transfer failed" });
        }
        Some(Commands::Finn { port, swarm_port, bootstrap }) | Some(Commands::Coop { port, swarm_port, bootstrap }) | Some(Commands::Run { port, swarm_port, bootstrap }) => run_node(port, swarm_port, bootstrap).await?,
        None => run_node(cli.port, cli.swarm_port, cli.bootstrap).await?,
    }
    Ok(())
}

async fn run_node(port: u16, swarm_port: u16, bootstrap: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let ledger = Ledger::new(&format!("primetime_db_{}", port));
    let mut p2p_node = P2PNode::new()?;
    p2p_node.swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{}", swarm_port).parse()?)?;
    let local_peer_id = p2p_node.peer_id.to_string();
    println!("Node ID: {}", local_peer_id);

    if let Some(boot_addr) = bootstrap {
        let multiaddr: Multiaddr = boot_addr.parse()?;
        p2p_node.swarm.dial(multiaddr.clone())?;
        if let Some(libp2p::multiaddr::Protocol::P2p(peer)) = multiaddr.iter().last() { p2p_node.swarm.behaviour_mut().kademlia.add_address(&peer, multiaddr); }
    }
    let _ = p2p_node.swarm.behaviour_mut().kademlia.bootstrap();

    let local_state = Arc::new(tokio::sync::RwLock::new(ledger.get_local_state(&local_peer_id)));
    let last_sim_update = Arc::new(tokio::sync::RwLock::new(None));
    let mempool = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let (tx, _rx) = broadcast::channel(100);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(100);

    let server_state = Arc::new(ServerState {
        tx: tx.clone(), ledger: ledger.clone(), event_tx: event_tx.clone(),
        local_peer_id: local_peer_id.clone(), keypair: p2p_node.keypair.clone(),
        local_state: local_state.clone(), last_sim_update: last_sim_update.clone(),
        mempool: mempool.clone(),
    });
    tokio::spawn(async move { start_server(server_state, port).await; });

    let hash_rate = Arc::new(std::sync::atomic::AtomicU64::new(0));
    
    let miner_state = local_state.clone();
    let miner_ledger = ledger.clone();
    let miner_peer_id = local_peer_id.clone();
    let miner_event_tx = event_tx.clone();
    let miner_hash_rate = hash_rate.clone();

    // DYNAMIC CHRONOS POH MINER
    tokio::spawn(async move {
        let mut current_pow_batch = 5000; 
        loop {
            let state_snapshot = miner_state.read().await.clone();
            let threshold = state_snapshot.active_heuristic.map_or(CONFIG.standard_mint_scarcity, |h| crate::core::STANDARDS[&h].mint_scarcity);

            let mut has_fuel = false;
            let mut fuel_updated_state = state_snapshot.clone();

            // THE COLD RESET (Explicit Safe Book Burn)
            if fuel_updated_state.active_book_counts.unwrap_or(0) > 0 {
                has_fuel = true;
            } else if fuel_updated_state.vault_books > 0 {
                fuel_updated_state.vault_books -= 1;
                fuel_updated_state.active_book_counts = Some(CONFIG.total_book_counts as usize);
                has_fuel = true;
                miner_ledger.save_local_state(&miner_peer_id, &fuel_updated_state);
                *miner_state.write().await = fuel_updated_state.clone();
            }

            if has_fuel {
                let pending_txs = {
                    let mut pool = mempool.lock().await;
                    let txs = pool.clone();
                    pool.clear();
                    txs
                };

                let last_hash = fuel_updated_state.last_hash;
                let pow_batch_copy = current_pow_batch;
                let start_time = Instant::now();
                
                let proof = tokio::task::spawn_blocking(move || mine_iterations(&last_hash, pow_batch_copy, pending_txs)).await.unwrap();
                
                let elapsed_ms = start_time.elapsed().as_millis() as usize;
                if elapsed_ms > 0 {
                    miner_hash_rate.store((pow_batch_copy as f64 / (elapsed_ms as f64 / 1000.0)) as u64, std::sync::atomic::Ordering::Relaxed);
                    current_pow_batch = (current_pow_batch as f64 * (2000.0 / elapsed_ms as f64).clamp(0.8, 1.2)) as usize;
                    current_pow_batch = current_pow_batch.max(100); 
                }

                let mut event_to_broadcast = None;
                let mut latest_state = miner_state.read().await.clone(); 
                
                latest_state.last_hash = proof.end_hash;
                latest_state.counts += 1;
                if let Some(ref mut fuel) = latest_state.active_book_counts { *fuel = fuel.saturating_sub(1); }
                latest_state.update_prime_value();

                if latest_state.prime_value >= threshold {
                    let counts_consumed = latest_state.active_heuristic.map_or(CONFIG.total_book_counts as usize, |h| crate::core::STANDARDS[&h].mint_counts);
                    let efficiency_gain = CONFIG.total_book_counts as i64 - counts_consumed as i64;

                    event_to_broadcast = Some(Event {
                        id: rand::random(),
                        parent_ids: vec![last_hash],
                        event_type: EventType::Mint { proof, heuristic: latest_state.active_heuristic },
                        entropy_delta: efficiency_gain.max(0),
                        state_root: *miner_ledger.state_root.read().unwrap(),
                    });
                } else if !proof.injected_txs.is_empty() {
                    // SILENT CHRONOMETER RULE: Only broadcast if Mempool TXs were injected
                    event_to_broadcast = Some(Event {
                        id: rand::random(),
                        parent_ids: vec![last_hash],
                        event_type: EventType::Tick { proof },
                        entropy_delta: 0,
                        state_root: *miner_ledger.state_root.read().unwrap(),
                    });
                }

                miner_ledger.save_local_state(&miner_peer_id, &latest_state);

                if let Some(ev) = event_to_broadcast {
                    if miner_ledger.verify_and_process_event(&ev) {
                        let _ = miner_event_tx.send(ev.clone()).await;
                        
                        let mut post_event_state = miner_ledger.get_local_state(&miner_peer_id);
                        if matches!(ev.event_type, EventType::Mint { .. }) {
                            let counts_consumed = post_event_state.active_heuristic.map_or(CONFIG.total_book_counts as usize, |h| crate::core::STANDARDS[&h].mint_counts);
                            post_event_state.vault_books += 1;
                            post_event_state.counts = 1.max(post_event_state.counts.saturating_sub(counts_consumed));
                            post_event_state.balance_adjustment = 0;
                            post_event_state.active_heuristic = None;
                            post_event_state.update_prime_value();
                            miner_ledger.save_local_state(&miner_peer_id, &post_event_state);
                        }
                        *miner_state.write().await = post_event_state;
                    }
                } else {
                    *miner_state.write().await = latest_state;
                }
            } else {
                miner_hash_rate.store(0, std::sync::atomic::Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    });

    let mut ui_interval = interval(Duration::from_millis(500));
    let mut tick = 0;
    let mut nat_status = "Unknown".to_string();
    let mut external_addrs = Vec::new();

    loop {
        tokio::select! {
            _ = ui_interval.tick() => {
                let state_clone = local_state.read().await.clone();
                let net_entropy = ledger.net_entropy.load(std::sync::atomic::Ordering::Relaxed);
                let voids = ledger.void_events.load(std::sync::atomic::Ordering::Relaxed);
                let surplus = ledger.surplus_events.load(std::sync::atomic::Ordering::Relaxed);
                
                let mut unit_precedents = std::collections::HashMap::new();
                for (u, std) in crate::core::STANDARDS.iter() { unit_precedents.insert(format!("{:?}", u), std.precedent); }

                let update = SimUpdate {
                    tick, node_id: local_peer_id.clone(), net_entropy, void_events: voids, surplus_events: surplus,
                    peer_count: p2p_node.swarm.network_info().num_peers(),
                    local_vault_books: state_clone.vault_books, local_prime_value: state_clone.prime_value,
                    local_counts: state_clone.counts, local_nonce: state_clone.nonce,
                    unit_precedents, hash_rate: hash_rate.load(std::sync::atomic::Ordering::Relaxed),
                    active_heuristic: state_clone.active_heuristic,
                    recent_transfers: ledger.get_recent_transfers(8),
                    external_addrs: external_addrs.clone(), nat_status: nat_status.clone(),
                    state_root: hex::encode(*ledger.state_root.read().unwrap()),
                };
                
                *last_sim_update.write().await = Some(update.clone());
                let _ = tx.send(serde_json::to_string(&update).unwrap());
                tick += 1;
            }

            Some(event) = event_rx.recv() => {
                let serialized_event = bincode::serialize(&event).unwrap();
                let topic = libp2p::gossipsub::IdentTopic::new("primetime-dag");
                let _ = p2p_node.swarm.behaviour_mut().gossipsub.publish(topic, serialized_event);
            }

            event = p2p_node.swarm.select_next_some() => {
                match event {
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        p2p_node.swarm.behaviour_mut().sync_req_resp.send_request(&peer_id, SyncRequest { from_tick: 0 });
                    },
                    SwarmEvent::ExternalAddrConfirmed { address } => external_addrs.push(address.to_string()),
                    SwarmEvent::Behaviour(P2PBehaviourEvent::Autonat(autonat::Event::StatusChanged { new, .. })) => nat_status = format!("{:?}", new),
                    SwarmEvent::Behaviour(P2PBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                        for (peer_id, multiaddr) in list {
                            p2p_node.swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddr.clone());
                            let _ = p2p_node.swarm.dial(multiaddr);
                        }
                    },
                    SwarmEvent::Behaviour(P2PBehaviourEvent::Identify(identify::Event::Received { peer_id, info })) => {
                        for addr in info.listen_addrs { p2p_node.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr); }
                    },
                    SwarmEvent::Behaviour(P2PBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. })) => {
                        if let Ok(e) = bincode::deserialize::<Event>(&message.data) {
                            if ledger.verify_and_process_event(&e) {
                                *local_state.write().await = ledger.get_local_state(&local_peer_id);
                            }
                        }
                    },
                    SwarmEvent::Behaviour(P2PBehaviourEvent::SyncReqResp(request_response::Event::Message { message, .. })) => {
                        match message {
                            request_response::Message::Request { channel, .. } => {
                                let _ = p2p_node.swarm.behaviour_mut().sync_req_resp.send_response(channel, SyncResponse { events: ledger.get_all_events() });
                            },
                            request_response::Message::Response { response, .. } => {
                                for e in response.events { ledger.verify_and_process_event(&e); }
                                *local_state.write().await = ledger.get_local_state(&local_peer_id);
                            }
                        }
                    },
                    _ => {}
                }
            }
        }
    }
}