
use yew::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use std::collections::HashMap;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = listen)]
    async fn listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> JsValue;
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EventType {
    pub transfer: Option<TransferEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TransferEvent {
    pub sender: String,
    pub receiver: String,
    pub amount_prime: u64,
    pub heuristic: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    pub id: String,
    pub event_type: EventType,
    pub entropy_delta: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct SimUpdate {
    pub tick: u64, pub node_id: String, pub net_entropy: i64, pub void_events: i64, pub surplus_events: i64,
    pub peer_count: usize, pub local_vault_books: u64, pub local_prime_value: u64, pub local_counts: usize,
    pub local_nonce: u64, pub unit_precedents: HashMap<String, u64>, pub hash_rate: u64,
    pub active_heuristic: Option<String>, pub recent_transfers: Vec<Event>,
    pub external_addrs: Vec<String>, pub nat_status: String,
    pub state_root: String,
}

#[derive(Serialize, Deserialize)]
struct TransferArgs {
    to: String,
    amount: u64,
    unit: String,
}

#[derive(Properties, PartialEq)]
pub struct DashboardProps {
    pub state: SimUpdate,
}

#[function_component(App)]
fn app() -> Html {
    let state = use_state(SimUpdate::default);

    {
        let state_setter = state.clone();
        use_effect_with((), move |_| {
            let state_setter_clone = state_setter.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let res = invoke("get_state", JsValue::NULL).await;
                if let Ok(initial) = serde_wasm_bindgen::from_value(res) {
                    state_setter_clone.set(initial);
                }
            });

            let closure = Closure::wrap(Box::new(move |payload: JsValue| {
                if let Ok(js_obj) = js_sys::Reflect::get(&payload, &JsValue::from_str("payload")) {
                    if let Ok(update) = serde_wasm_bindgen::from_value::<SimUpdate>(js_obj) {
                        state_setter.set(update);
                    }
                }
            }) as Box<dyn FnMut(JsValue)>);

            wasm_bindgen_futures::spawn_local(async move {
                listen("sim_update", &closure).await;
                closure.forget();
            });
        });
    }

    let p_name = "FINN";
    let sms = 114_113; // Finn profile
    let pct = (state.local_prime_value as f64 / sms as f64 * 100.0).min(100.0);

    let active_h_display = state.active_heuristic.as_ref().map(|h| format!("HEURISTIC: {}", h)).unwrap_or_default();
    let status_badge = if state.hash_rate > 0 { "MINING PoTH" } else { "IDLE" };
    let status_class = if state.hash_rate > 0 { "text-[8px] px-1.5 py-0.5 rounded bg-emerald-500/10 text-emerald-400 font-bold uppercase" } else { "text-[8px] px-1.5 py-0.5 rounded bg-slate-500/10 text-slate-500 font-bold uppercase" };
    let agent_class = if state.hash_rate > 0 { "glass-panel p-6 rounded-2xl relative overflow-hidden active-standard border-slate-700" } else { "glass-panel p-6 rounded-2xl relative overflow-hidden border-slate-700" };

    let state_root_display = if state.state_root.len() > 16 { format!("{}...", &state.state_root[0..16]) } else { state.state_root.clone() };

    // Transfer State
    let tx_to = use_state(|| String::new());
    let tx_amount = use_state(|| String::new());
    let tx_unit = use_state(|| String::new());
    let tx_status = use_state(|| String::new());

    let on_to_change = { let tx_to = tx_to.clone(); Callback::from(move |e: yew::Event| { let input: web_sys::HtmlInputElement = e.target_unchecked_into(); tx_to.set(input.value()); }) };
    let on_amount_change = { let tx_amount = tx_amount.clone(); Callback::from(move |e: yew::Event| { let input: web_sys::HtmlInputElement = e.target_unchecked_into(); tx_amount.set(input.value()); }) };
    let on_unit_change = { let tx_unit = tx_unit.clone(); Callback::from(move |e: yew::Event| { let input: web_sys::HtmlSelectElement = e.target_unchecked_into(); tx_unit.set(input.value()); }) };

    let on_transfer = {
        let tx_to = tx_to.clone();
        let tx_amount = tx_amount.clone();
        let tx_unit = tx_unit.clone();
        let tx_status = tx_status.clone();
        let fallback_unit = "Day".to_string(); // Default if unit is empty

        Callback::from(move |_| {
            let to = (*tx_to).clone();
            let amount = (*tx_amount).parse::<u64>().unwrap_or(0);
            let unit = if (*tx_unit).is_empty() { fallback_unit.clone() } else { (*tx_unit).clone() };
            let status = tx_status.clone();

            if to.is_empty() || amount == 0 {
                status.set("<span class='text-rose-400'>Error: Missing fields</span>".to_string());
                return;
            }

            status.set("<span class='text-yellow-400'>Queuing in Mempool...</span>".to_string());
            wasm_bindgen_futures::spawn_local(async move {
                let args = TransferArgs { to, amount, unit };
                if let Ok(val) = serde_wasm_bindgen::to_value(&args) {
                    let res = invoke("do_transfer", val).await;
                    if let Ok(msg) = serde_wasm_bindgen::from_value::<String>(res) {
                        status.set(format!("<span class='text-emerald-400'>{}</span>", msg));
                    } else {
                        status.set("<span class='text-rose-400'>Failed to parse response</span>".to_string());
                    }
                }
            });
        })
    };

    let day_val = *state.unit_precedents.get("Day").unwrap_or(&1);
    let deg_val = *state.unit_precedents.get("Degree").unwrap_or(&1);
    let twin_val = *state.unit_precedents.get("Twin").unwrap_or(&1);

    let pv = state.local_prime_value;
    let days = pv / day_val;
    let mut rem = pv % day_val;
    let degrees = rem / deg_val;
    rem = rem % deg_val;
    let twins = rem / twin_val;

    html! {
        <div class="max-w-[1400px] mx-auto space-y-8">
            <header class="flex justify-between items-center border-b border-slate-800 pb-6">
                <div>
                    <h1 class="text-2xl font-black tracking-tighter text-white">{"PRIME-TIME "} <span class="text-xs bg-cyan-500/10 text-cyan-400 px-2 py-1 rounded border border-cyan-500/20 font-mono">{"MOBILE NODE"}</span></h1>
                    <p class="text-[10px] text-slate-500 tracking-widest uppercase mt-1 font-bold">{format!("Byte-Pure PoH Engine v8.0 | PROFILE: {}", p_name)}</p>
                    <p class="text-[8px] text-slate-600 font-mono mt-1">{"STATE ROOT: "} <span>{state_root_display}</span></p>
                </div>
                <div class="flex gap-4 text-right font-mono">
                    <div><p class="text-[10px] text-slate-500 uppercase">{"System Tick"}</p><p class="text-sm font-bold text-white">{state.tick}</p></div>
                    <div><p class="text-[10px] text-slate-500 uppercase">{"Peers"}</p><p class="text-sm font-bold text-cyan-400">{state.peer_count}</p></div>
                </div>
            </header>

            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-5 gap-4">
                <div class="glass-panel p-4 rounded-xl border-l-2 border-l-rose-500/50">
                    <p class="text-[10px] text-slate-500 uppercase font-bold">{"Voids Detected"}</p><p class="text-xl font-bold text-white font-mono">{state.void_events}</p>
                </div>
                <div class="glass-panel p-4 rounded-xl border-l-2 border-l-emerald-500/50">
                    <p class="text-[10px] text-slate-500 uppercase font-bold">{"Surplus Mints"}</p><p class="text-xl font-bold text-white font-mono">{state.surplus_events}</p>
                </div>
                <div class="glass-panel p-4 rounded-xl border-l-2 border-l-cyan-500/50">
                    <p class="text-[10px] text-slate-500 uppercase font-bold">{"Net Entropy"}</p><p class="text-xl font-bold text-white font-mono">{state.net_entropy}</p>
                </div>
                <div class="glass-panel p-4 rounded-xl border-l-2 border-l-yellow-500/50">
                    <p class="text-[10px] text-slate-500 uppercase font-bold">{"PoTH Hash Rate"}</p><p class="text-xl font-bold text-white font-mono"><span>{state.hash_rate}</span> <span class="text-[10px] text-yellow-500">{" H/S"}</span></p>
                </div>
                <div class="glass-panel p-4 rounded-xl border-l-2 border-l-purple-500/50">
                    <p class="text-[10px] text-slate-500 uppercase font-bold">{"NAT / Reachability"}</p><p class="text-sm font-bold text-cyan-400 font-mono truncate">{&state.nat_status}</p>
                </div>
            </div>

            <div class="grid grid-cols-1 xl:grid-cols-3 gap-8">
                <div class="xl:col-span-1 space-y-4">
                    <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest">{"Local Node Agent"}</h3>
                    <div class={agent_class}>
                        <div class="absolute top-0 left-0 h-1 bg-cyan-400 transition-all duration-300" style={format!("width: {}%", pct)}></div>
                        <div class="flex justify-between items-start mb-6">
                            <div>
                                <div class="flex items-center gap-2">
                                    <h4 class="text-lg font-black text-white tracking-tight">{"ALPHA_NODE"}</h4>
                                    <span class="text-[10px] bg-cyan-500/10 text-cyan-400 px-1.5 py-0.5 rounded border border-cyan-500/20 font-mono">{"ED25519"}</span>
                                </div>
                                <div class="text-[10px] font-bold mt-1 text-purple-400 uppercase">{active_h_display}</div>
                                <div class="flex items-center gap-2 mt-1"><span class={status_class}>{status_badge}</span></div>
                            </div>
                            <div class="text-right"><p class="text-[8px] text-slate-500 uppercase font-bold">{"Nonce"}</p><p class="text-xs font-bold text-white font-mono">{state.local_nonce}</p></div>
                        </div>
                        <div class="space-y-4">
                            <div class="bg-slate-900/50 p-4 rounded-xl border border-slate-800">
                                <p class="text-[10px] text-slate-500 uppercase font-bold mb-1">{"Scarcity Index (Prime Value)"}</p>
                                <p class="text-2xl font-black text-white font-mono tracking-tighter">{state.local_prime_value}</p>
                            </div>
                            <div class="bg-slate-900/50 p-4 rounded-xl border border-slate-800">
                                <p class="text-[10px] text-slate-500 uppercase font-bold mb-2">{"Vault Balance"}</p>
                                <div class="flex items-baseline gap-2"><p class="text-xl font-bold text-emerald-400 font-mono">{state.local_vault_books}</p><span class="text-xs text-slate-500 font-bold uppercase">{"Books"}</span></div>
                            </div>
                        </div>
                    </div>

                    <div class="glass-panel p-6 rounded-2xl border-slate-700 mt-4">
                        <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest mb-4">{"Manual Transfer Terminal"}</h3>
                        <div class="space-y-3">
                            <div>
                                <label class="text-[10px] text-slate-500 uppercase font-bold">{"Target Peer ID"}</label>
                                <input type="text" value={(*tx_to).clone()} onchange={on_to_change} class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500" placeholder="e.g. 12D3KooW..." />
                            </div>
                            <div class="grid grid-cols-2 gap-3">
                                <div>
                                    <label class="text-[10px] text-slate-500 uppercase font-bold">{"PV Amount"}</label>
                                    <input type="number" value={(*tx_amount).clone()} onchange={on_amount_change} class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500" placeholder="e.g. 5000" />
                                </div>
                                <div>
                                    <label class="text-[10px] text-slate-500 uppercase font-bold">{"Heuristic"}</label>
                                    <select onchange={on_unit_change} class="w-full bg-slate-900/50 border border-slate-700 rounded p-2 text-sm text-white font-mono outline-none focus:border-cyan-500">
                                        { for state.unit_precedents.keys().map(|u| html! { <option value={u.clone()} selected={*tx_unit == *u}>{u.clone()}</option> }) }
                                    </select>
                                </div>
                            </div>
                            <button onclick={on_transfer} class="w-full bg-cyan-500/20 hover:bg-cyan-500/40 border border-cyan-500/50 text-cyan-400 font-bold uppercase text-xs py-2 rounded transition-colors">{"Queue into PoH Mempool"}</button>
                            <div class="text-[10px] font-mono mt-2 text-center h-4" dangerously_set_inner_html={(*tx_status).clone()}></div>
                        </div>
                    </div>

                    <div class="space-y-4 pt-4">
                        <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest">{"PoH Verified Transfers"}</h3>
                        <div class="glass-panel rounded-xl overflow-hidden divide-y divide-slate-800">
                            { for state.recent_transfers.iter().map(|e| {
                                if let Some(t) = &e.event_type.transfer {
                                    let sign = if e.entropy_delta > 0 { "+" } else { "" };
                                    let delta_class = if e.entropy_delta > 0 { "text-emerald-400" } else { "text-rose-400" };
                                    html! {
                                        <div class="p-3 text-[10px] font-mono flex justify-between items-center bg-slate-900/30">
                                            <div class="space-y-1">
                                                <div><span class="text-slate-500">{"From: "}</span><span class="text-white">{&t.sender.chars().take(8).collect::<String>()} {"..."}</span></div>
                                                <div><span class="text-slate-500">{"To: "}</span><span class="text-white">{&t.receiver.chars().take(8).collect::<String>()} {"..."}</span></div>
                                            </div>
                                            <div class="text-right">
                                                <p class="text-cyan-400 font-bold">{&t.heuristic}</p>
                                                <p class={delta_class}>{format!("ΔS: {}{}", sign, e.entropy_delta)}</p>
                                            </div>
                                        </div>
                                    }
                                } else { html! {} }
                            })}
                        </div>
                    </div>
                </div>

                <div class="xl:col-span-2 space-y-4">
                    <h3 class="text-xs font-bold text-slate-500 uppercase tracking-widest">{"Global Entropy Analytics"}</h3>
                    <div class="glass-panel p-6 rounded-2xl h-[400px] flex items-center justify-center text-slate-500 text-sm">
                        {"(Chart.js hidden in mobile view / Yew port)"}
                    </div>
                </div>
            </div>

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
        </div>
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
