use yew::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"])]
    async fn listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> JsValue;
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SimUpdate {
    pub tick: u64, pub node_id: String, pub net_entropy: i64, pub void_events: i64, pub surplus_events: i64,
    pub peer_count: usize, pub local_vault_books: u64, pub local_prime_value: u64, pub local_counts: usize,
    pub local_nonce: u64, pub unit_precedents: std::collections::HashMap<String, u64>, pub hash_rate: u64,
    pub active_heuristic: Option<String>, pub recent_transfers: Vec<Event>,
    pub external_addrs: Vec<String>, pub nat_status: String,
    pub state_root: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: [u8; 32],
    pub parent_ids: Vec<[u8; 32]>,
    pub event_type: EventType,
    pub entropy_delta: i64,
    pub state_root: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    Mint { proof: serde_json::Value, heuristic: Option<String> },
    Tick { proof: serde_json::Value },
    Transfer { sender: String, sender_pk: Vec<u8>, receiver: String, amount_prime: u64, heuristic: String, nonce: u64, signature: Vec<u8> },
}


#[function_component(App)]
fn app() -> Html {
    let state = use_state(|| None::<SimUpdate>);

    {
        let state = state.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let handler = Closure::wrap(Box::new(move |event: JsValue| {
                    if let Ok(payload) = js_sys::Reflect::get(&event, &JsValue::from_str("payload")) {
                        if let Ok(update) = serde_wasm_bindgen::from_value::<SimUpdate>(payload) {
                            state.set(Some(update));
                        }
                    }
                }) as Box<dyn FnMut(JsValue)>);

                listen("arithmodynamic_tick", &handler).await;
                handler.forget();
            });
            || ()
        });
    }

    if let Some(s) = (*state).clone() {
        html! {
            <div class="p-4 md:p-8 min-h-screen pb-32">
                <div class="max-w-[1400px] mx-auto space-y-8">
                    <header class="flex justify-between items-center border-b border-slate-800 pb-6">
                        <div>
                            <h1 class="text-2xl font-black tracking-tighter text-white">{"PRIME-TIME "} <span class="text-xs bg-cyan-500/10 text-cyan-400 px-2 py-1 rounded border border-cyan-500/20 font-mono">{"NODE OPERATOR"}</span></h1>
                            <p class="text-[10px] text-slate-500 tracking-widest uppercase mt-1 font-bold">{"Byte-Pure PoH Engine v8.0 | PROFILE: FINN"}</p>
                            <p class="text-[8px] text-slate-600 font-mono mt-1">{"STATE ROOT: "} <span>{s.state_root[0..16].to_string()}{"..."}</span></p>
                        </div>
                        <div class="flex gap-4 text-right font-mono">
                            <div><p class="text-[10px] text-slate-500 uppercase">{"System Tick"}</p><p class="text-sm font-bold text-white">{s.tick}</p></div>
                            <div><p class="text-[10px] text-slate-500 uppercase">{"Peers"}</p><p class="text-sm font-bold text-cyan-400">{s.peer_count}</p></div>
                        </div>
                    </header>
                    <Dashboard state={s.clone()} />
                    <div class="grid grid-cols-1 xl:grid-cols-3 gap-8">
                        <div class="xl:col-span-1 space-y-4">
                            <AgentCard state={s.clone()} />
                            <TransferTerminal />
                            <EventLog state={s.clone()} />
                        </div>
                    </div>
                </div>
                <StickyClockFooter state={s} />
            </div>
        }
    } else {
        html! { <div class="text-white p-8">{"Connecting to Arithmodynamic Engine..."}</div> }
    }
}

#[derive(Properties, PartialEq)]
struct StateProps {
    state: SimUpdate,
}

#[function_component(Dashboard)]
fn dashboard(props: &StateProps) -> Html {
    let s = &props.state;
    html! {
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-4">
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-rose-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">{"Voids Detected"}</p><p class="text-xl font-bold text-white font-mono">{s.void_events}</p>
            </div>
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-emerald-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">{"Surplus Mints"}</p><p class="text-xl font-bold text-white font-mono">{s.surplus_events}</p>
            </div>
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-cyan-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">{"Net Entropy"}</p><p class="text-xl font-bold text-white font-mono">{s.net_entropy}</p>
            </div>
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-yellow-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">{"PoTH Hash Rate"}</p><p class="text-xl font-bold text-white font-mono"><span>{s.hash_rate}</span> <span class="text-[10px] text-yellow-500">{"H/S"}</span></p>
            </div>
            <div class="glass-panel p-4 rounded-xl border-l-2 border-l-purple-500/50">
                <p class="text-[10px] text-slate-500 uppercase font-bold">{"NAT / Reachability"}</p><p class="text-sm font-bold text-cyan-400 font-mono truncate">{&s.nat_status}</p>
            </div>
        </div>
    }
}

#[function_component(AgentCard)]
fn agent_card(props: &StateProps) -> Html {
    let s = &props.state;
    let pct = (s.local_prime_value as f64 / 114_113.0 * 100.0).min(100.0);
    html! {
        <>
            <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest">{"Local Node Agent"}</h3>
            <div class={if s.hash_rate > 0 { "glass-panel p-6 rounded-2xl relative overflow-hidden active-standard border-slate-700" } else { "glass-panel p-6 rounded-2xl relative overflow-hidden border-slate-700" }}>
                <div class="absolute top-0 left-0 h-1 bg-cyan-400 transition-all duration-300" style={format!("width: {}%", pct)}></div>
                <div class="flex justify-between items-start mb-6">
                    <div>
                        <div class="flex items-center gap-2">
                            <h4 class="text-lg font-black text-white tracking-tight">{"ALPHA_NODE"}</h4>
                            <span class="text-[10px] bg-cyan-500/10 text-cyan-400 px-1.5 py-0.5 rounded border border-cyan-500/20 font-mono">{"ED25519"}</span>
                        </div>
                        <div class="text-[10px] font-bold mt-1 text-purple-400 uppercase">{s.active_heuristic.as_ref().map(|h| format!("HEURISTIC: {}", h)).unwrap_or_default()}</div>
                        <div class="flex items-center gap-2 mt-1"><span class={if s.hash_rate > 0 { "text-[8px] px-1.5 py-0.5 rounded bg-emerald-500/10 text-emerald-400 font-bold uppercase" } else { "text-[8px] px-1.5 py-0.5 rounded bg-slate-500/10 text-slate-500 font-bold uppercase" }}>{if s.hash_rate > 0 {"MINING PoTH"} else {"IDLE"}}</span></div>
                    </div>
                    <div class="text-right"><p class="text-[8px] text-slate-500 uppercase font-bold">{"Nonce"}</p><p class="text-xs font-bold text-white font-mono">{s.local_nonce}</p></div>
                </div>
                <div class="space-y-4">
                    <div class="bg-slate-900/50 p-4 rounded-xl border border-slate-800">
                        <p class="text-[10px] text-slate-500 uppercase font-bold mb-1">{"Scarcity Index (Prime Value)"}</p>
                        <p class="text-2xl font-black text-white font-mono tracking-tighter">{s.local_prime_value}</p>
                    </div>
                    <div class="bg-slate-900/50 p-4 rounded-xl border border-slate-800">
                        <p class="text-[10px] text-slate-500 uppercase font-bold mb-2">{"Vault Balance"}</p>
                        <div class="flex items-baseline gap-2"><p class="text-xl font-bold text-emerald-400 font-mono">{s.local_vault_books}</p><span class="text-xs text-slate-500 font-bold uppercase">{"Books"}</span></div>
                    </div>
                </div>
            </div>
        </>
    }
}

#[derive(Serialize)]
struct TransferArgs {
    to: String, amount: u64, unit: String,
}

#[function_component(TransferTerminal)]
fn transfer_terminal() -> Html {
    let to_ref = use_node_ref();
    let amount_ref = use_node_ref();
    let unit_ref = use_node_ref();
    let status = use_state(String::new);

    let onsubmit = {
        let to_ref = to_ref.clone();
        let amount_ref = amount_ref.clone();
        let unit_ref = unit_ref.clone();
        let status = status.clone();

        Callback::from(move |_| {
            let to = to_ref.cast::<web_sys::HtmlInputElement>().unwrap().value();
            let amount = amount_ref.cast::<web_sys::HtmlInputElement>().unwrap().value().parse::<u64>().unwrap_or(0);
            let unit = unit_ref.cast::<web_sys::HtmlSelectElement>().unwrap().value();
            let status = status.clone();

            spawn_local(async move {
                if to.is_empty() || amount == 0 {
                    status.set("Error: Missing fields".to_string());
                    return;
                }
                
                let args = serde_wasm_bindgen::to_value(&TransferArgs { to, amount, unit }).unwrap();
                status.set("Queuing...".to_string());
                match invoke("do_transfer", args).await.as_string() {
                    Some(res) => status.set(res),
                    None => status.set("Error: Failed to invoke".to_string()),
                }
            });
        })
    };

    html! {
        <div class="glass-panel p-6 rounded-2xl border-slate-700 mt-4">
            <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest mb-4">{"Manual Transfer Terminal"}</h3>
            <div class="space-y-3">
                <div>
                    <label class="text-[10px] text-slate-500 uppercase font-bold">{"Target Peer ID"}</label>
                    <input ref={to_ref} type="text" class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500" placeholder="e.g. 12D3KooW..." />
                </div>
                <div class="grid grid-cols-2 gap-3">
                    <div>
                        <label class="text-[10px] text-slate-500 uppercase font-bold">{"PV Amount"}</label>
                        <input ref={amount_ref} type="number" class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500" placeholder="e.g. 5000" />
                    </div>
                    <div>
                        <label class="text-[10px] text-slate-500 uppercase font-bold">{"Heuristic"}</label>
                        <select ref={unit_ref} class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500">
                            <option value="Day">{"Day"}</option>
                            <option value="Degree">{"Degree"}</option>
                            <option value="Twin">{"Twin"}</option>
                        </select>
                    </div>
                </div>
                <button onclick={onsubmit} class="w-full bg-cyan-500/20 hover:bg-cyan-500/40 border border-cyan-500/50 text-cyan-400 font-bold uppercase text-xs py-2 rounded transition-colors">{"Queue into PoH Mempool"}</button>
                <div class="text-[10px] font-mono mt-2 text-center h-4 text-emerald-400">{&*status}</div>
            </div>
        </div>
    }
}

#[function_component(EventLog)]
fn event_log(props: &StateProps) -> Html {
    let s = &props.state;
    html! {
        <div class="space-y-4 pt-4">
            <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest">{"PoH Verified Transfers"}</h3>
            <div class="glass-panel rounded-xl overflow-hidden divide-y divide-slate-800">
                {for s.recent_transfers.iter().map(|e| {
                    if let EventType::Transfer { sender, receiver, heuristic, .. } = &e.event_type {
                        html! {
                            <div class="p-3 text-[10px] font-mono flex justify-between items-center bg-slate-900/30">
                                <div class="space-y-1">
                                    <div><span class="text-slate-500">{"From: "}</span> <span class="text-white">{&sender[0..8]}{"..."}</span></div>
                                    <div><span class="text-slate-500">{"To: "}</span> <span class="text-white">{&receiver[0..8]}{"..."}</span></div>
                                </div>
                                <div class="text-right">
                                    <p class="text-cyan-400 font-bold">{heuristic}</p>
                                    <p class={if e.entropy_delta > 0 { "text-emerald-400" } else { "text-rose-400" }}>{format!("ΔS: {:+}", e.entropy_delta)}</p>
                                </div>
                            </div>
                        }
                    } else {
                        html! {}
                    }
                })}
            </div>
        </div>
    }
}

#[function_component(StickyClockFooter)]
fn sticky_clock_footer(props: &StateProps) -> Html {
    let s = &props.state;
    let pv = s.local_prime_value;
    let day_val = s.unit_precedents.get("Day").copied().unwrap_or(1);
    let deg_val = s.unit_precedents.get("Degree").copied().unwrap_or(1);
    let twin_val = s.unit_precedents.get("Twin").copied().unwrap_or(1);

    let days = pv / day_val;
    let mut rem = pv % day_val;
    let degrees = rem / deg_val;
    rem %= deg_val;
    let twins = rem / twin_val;

    html! {
        <footer class="fixed bottom-6 left-1/2 -translate-x-1/2 glass-panel px-8 py-4 rounded-[18px] z-50 border-cyan-500/30 shadow-[0_0_30px_rgba(0,0,0,0.5)] min-w-[320px]">
            <div class="flex justify-between items-center gap-12">
                <div class="text-center">
                    <p class="text-[9px] text-slate-500 uppercase font-black tracking-tighter mb-1">{"Days"}</p>
                    <p class="text-2xl font-black text-white font-mono leading-none">{format!("{:02}", days)}</p>
                </div>
                <div class="h-8 w-px bg-slate-800"></div>
                <div class="text-center">
                    <p class="text-[9px] text-slate-500 uppercase font-black tracking-tighter mb-1">{"Degrees"}</p>
                    <p class="text-2xl font-black text-cyan-400 font-mono leading-none">{format!("{:02}", degrees)}</p>
                </div>
                <div class="h-8 w-px bg-slate-800"></div>
                <div class="text-center">
                    <p class="text-[9px] text-slate-500 uppercase font-black tracking-tighter mb-1">{"Twins"}</p>
                    <p class="text-2xl font-black text-emerald-400 font-mono leading-none">{format!("{:02}", twins)}</p>
                </div>
            </div>
        </footer>
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
