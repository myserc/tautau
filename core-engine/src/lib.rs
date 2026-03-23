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
            let mode = "finn".to_string(); // Force finn mode for mobile to avoid OOM
            Config {
                mode: "finn".to_string(), limit: 1_200_000, total_book_counts: 10_800, standard_mint_scarcity: 114_113,
                units: vec![Unit::Day, Unit::Degree, Unit::Twin], num_agents: 250_000,
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
        pub db: Db,
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
                db,
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
