use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proof {
    #[serde(with = "crate::utils::serde_hex")] pub start_hash: [u8; 32],
    #[serde(with = "crate::utils::serde_hex")] pub end_hash: [u8; 32],
    pub iterations: usize,
    pub injected_txs: Vec<(usize, Vec<u8>)>,
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
