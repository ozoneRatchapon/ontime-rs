use dioxus::prelude::*;
use ontime_core::types::{PlaybackState, WsAction};

#[component]
pub fn ControlPanel(
    playback_state: PlaybackState,
    on_action: EventHandler<WsAction>,
) -> Element {
    let is_running = playback_state == PlaybackState::Running;

    rsx! {
        div { class: "flex items-center justify-center space-x-4 p-4 bg-slate-900/80 backdrop-blur-xl border border-slate-800 rounded-2xl max-w-2xl mx-auto shadow-2xl my-4",
            button {
                class: "px-4 py-2.5 bg-slate-800 hover:bg-slate-700 active:scale-95 text-white font-semibold rounded-xl border border-slate-700 transition-all shadow-md flex items-center space-x-2",
                onclick: move |_| on_action.call(WsAction::PreviousEvent),
                "⏮ Prev"
            }
            if is_running {
                button {
                    class: "px-6 py-2.5 bg-amber-600 hover:bg-amber-500 active:scale-95 text-white font-bold rounded-xl border border-amber-500 transition-all shadow-lg flex items-center space-x-2",
                    onclick: move |_| on_action.call(WsAction::PauseTimer),
                    "⏸ Pause"
                }
            } else {
                button {
                    class: "px-6 py-2.5 bg-emerald-600 hover:bg-emerald-500 active:scale-95 text-white font-bold rounded-xl border border-emerald-500 transition-all shadow-lg flex items-center space-x-2",
                    onclick: move |_| on_action.call(WsAction::StartTimer),
                    "▶ Play"
                }
            }
            button {
                class: "px-4 py-2.5 bg-rose-950/80 hover:bg-rose-900 active:scale-95 text-rose-300 font-semibold rounded-xl border border-rose-800/80 transition-all shadow-md flex items-center space-x-2",
                onclick: move |_| on_action.call(WsAction::ResetTimer),
                "⏹ Reset"
            }
            button {
                class: "px-4 py-2.5 bg-slate-800 hover:bg-slate-700 active:scale-95 text-white font-semibold rounded-xl border border-slate-700 transition-all shadow-md flex items-center space-x-2",
                onclick: move |_| on_action.call(WsAction::NextEvent),
                "Next ⏭"
            }
        }
    }
}
