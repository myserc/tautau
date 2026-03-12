pub mod config;
pub mod core;
pub mod ledger;
pub mod pow;
pub mod p2p;
pub mod server;
pub mod utils;

use tauri::{Manager, RunEvent, Emitter};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, Duration, Instant};
use libp2p::swarm::SwarmEvent;
use libp2p::{gossipsub, request_response, mdns, identify, autonat};
use libp2p::futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ledger::{Ledger, Event, EventType};
use crate::p2p::{P2PNode, P2PBehaviourEvent, SyncRequest, SyncResponse};
use crate::pow::mine_iterations;
use crate::server::{SimUpdate, ServerState, TransferRequest};
use crate::config::CONFIG;
use crate::core::CHAIN_ID;

static IS_MINING: AtomicBool = AtomicBool::new(true);

#[tauri::command]
async fn get_state(state: tauri::State<'_, Arc<ServerState>>) -> Result<SimUpdate, String> {
    let update = state.last_sim_update.read().await;
    Ok(update.clone().unwrap_or(SimUpdate {
        tick: 0, node_id: state.local_peer_id.clone(), net_entropy: 0, void_events: 0, surplus_events: 0, peer_count: 0,
        local_vault_books: 0, local_prime_value: 0, local_counts: 0, local_nonce: 0, unit_precedents: std::collections::HashMap::new(),
        hash_rate: 0, active_heuristic: None, recent_transfers: vec![], external_addrs: vec![], nat_status: "Unknown".to_string(),
        state_root: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    }))
}

#[tauri::command]
async fn do_transfer(to: String, amount: u64, unit: crate::core::Unit, state: tauri::State<'_, Arc<ServerState>>) -> Result<String, String> {
    let req = TransferRequest { to, amount, unit };
    let mut local_state = state.local_state.write().await;
    if local_state.prime_value < req.amount { return Err("Insufficient funds".to_string()); }

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

    Ok("Transfer pooled for local PoH Injection".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()

        .setup(|app| {
            let app_handle = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                let db_path = app_handle.path().app_local_data_dir().unwrap().join("primetime_db");
                let db_path_str = db_path.to_str().unwrap().to_string();
                std::fs::create_dir_all(&db_path).unwrap_or_default();

                let ledger = Ledger::new(&db_path_str);
                let mut p2p_node = P2PNode::new().expect("Failed to create p2p node");
                p2p_node.swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap()).expect("Failed to listen");
                let local_peer_id = p2p_node.peer_id.to_string();

                let local_state = Arc::new(tokio::sync::RwLock::new(ledger.get_local_state(&local_peer_id)));
                let last_sim_update = Arc::new(tokio::sync::RwLock::new(None));
                let mempool = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
                let (event_tx, mut event_rx) = mpsc::channel::<Event>(100);

                let server_state = Arc::new(ServerState {
                    ledger: ledger.clone(), event_tx: event_tx.clone(),
                    local_peer_id: local_peer_id.clone(), keypair: p2p_node.keypair.clone(),
                    local_state: local_state.clone(), last_sim_update: last_sim_update.clone(),
                    mempool: mempool.clone(),
                });
                app_handle.manage(server_state);

                let hash_rate = Arc::new(std::sync::atomic::AtomicU64::new(0));

                let miner_state = local_state.clone();
                let miner_ledger = ledger.clone();
                let miner_peer_id = local_peer_id.clone();
                let miner_event_tx = event_tx.clone();
                let miner_hash_rate = hash_rate.clone();

                tokio::spawn(async move {
                    let mut current_pow_batch = 5000;
                    loop {
                        if !IS_MINING.load(Ordering::Relaxed) {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            continue;
                        }

                        let state_snapshot = miner_state.read().await.clone();
                        let threshold = state_snapshot.active_heuristic.map_or(CONFIG.standard_mint_scarcity, |h| crate::core::STANDARDS[&h].mint_scarcity);

                        let mut has_fuel = false;
                        let mut fuel_updated_state = state_snapshot.clone();

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
                let app_handle_loop = app_handle.clone();

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
                            let _ = app_handle_loop.emit("sim_update", &update);

                            #[cfg(target_os = "android")]
                            {
                                // Write to Android SharedPreferences and emit intent here
                                let days = update.local_prime_value / update.unit_precedents.get("Day").copied().unwrap_or(1);
                                let mut rem = update.local_prime_value % update.unit_precedents.get("Day").copied().unwrap_or(1);
                                let degrees = rem / update.unit_precedents.get("Degree").copied().unwrap_or(1);
                                rem = rem % update.unit_precedents.get("Degree").copied().unwrap_or(1);
                                let twins = rem / update.unit_precedents.get("Twin").copied().unwrap_or(1);

                                let _ = write_to_android_widget(days, degrees, twins);
                            }

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
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_state, do_transfer])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| match event {
            RunEvent::WindowEvent { event: tauri::WindowEvent::Focused(focused), .. } => {
                IS_MINING.store(focused, Ordering::Relaxed);
            }
            _ => {}
        });
}

#[cfg(target_os = "android")]
fn write_to_android_widget(days: u64, degrees: u64, twins: u64) -> Result<(), Box<dyn std::error::Error>> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }?;
    let mut env = vm.attach_current_thread()?;

    let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };

    // Get SharedPreferences
    let pref_name = env.new_string("WidgetPrefs")?;
    let shared_prefs = env.call_method(
        context,
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        &[(&pref_name).into(), jni::objects::JValue::Int(0)]
    )?.l()?;

    // Get Editor
    let editor = env.call_method(shared_prefs, "edit", "()Landroid/content/SharedPreferences$Editor;", &[])?.l()?;

    // Put values
    let editor = env.call_method(editor, "putLong", "(Ljava/lang/String;J)Landroid/content/SharedPreferences$Editor;", &[(&env.new_string("days")?).into(), jni::objects::JValue::Long(days as i64)])?.l()?;
    let editor = env.call_method(editor, "putLong", "(Ljava/lang/String;J)Landroid/content/SharedPreferences$Editor;", &[(&env.new_string("degrees")?).into(), jni::objects::JValue::Long(degrees as i64)])?.l()?;
    let editor = env.call_method(editor, "putLong", "(Ljava/lang/String;J)Landroid/content/SharedPreferences$Editor;", &[(&env.new_string("twins")?).into(), jni::objects::JValue::Long(twins as i64)])?.l()?;

    // Apply
    env.call_method(editor, "apply", "()V", &[])?;

    // Send Intent to update widget
    let intent_class = env.find_class("android/content/Intent")?;
    let action_string = env.new_string("primetime.action.UPDATE_WIDGET")?;
    let intent = env.new_object(intent_class, "(Ljava/lang/String;)V", &[(&action_string).into()])?;

    env.call_method(context, "sendBroadcast", "(Landroid/content/Intent;)V", &[(&intent).into()])?;

    Ok(())
}
