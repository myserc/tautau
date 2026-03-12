use libp2p::{ gossipsub, kad, noise, swarm::NetworkBehaviour, tcp, yamux, PeerId, Swarm, request_response, mdns, autonat, relay, dcutr, identify };
use libp2p::kad::store::MemoryStore;
use libp2p::identity::Keypair;
use serde::{Deserialize, Serialize};
use std::time::Duration;
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

        let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
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
