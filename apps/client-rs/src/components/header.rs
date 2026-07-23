use dioxus::prelude::*;
use ontime_core::types::PlaybackState;

#[component]
pub fn Header(title: String, playback_state: PlaybackState) -> Element {
    let state_badge = match playback_state {
        PlaybackState::Running => rsx! { span { class: "badge bg-emerald-500/20 text-emerald-400 border border-emerald-500/30 px-3 py-1 rounded-full text-xs font-semibold uppercase tracking-wider", "LIVE" } },
        PlaybackState::Paused => rsx! { span { class: "badge bg-amber-500/20 text-amber-400 border border-amber-500/30 px-3 py-1 rounded-full text-xs font-semibold uppercase tracking-wider", "PAUSED" } },
        PlaybackState::Stopped => rsx! { span { class: "badge bg-slate-500/20 text-slate-400 border border-slate-500/30 px-3 py-1 rounded-full text-xs font-semibold uppercase tracking-wider", "STOPPED" } },
    };

    rsx! {
        header { class: "flex items-center justify-between px-6 py-4 bg-slate-900/80 backdrop-blur-md border-b border-slate-800 text-white shadow-xl",
            div { class: "flex items-center space-x-4",
                h1 { class: "text-2xl font-bold tracking-tight bg-gradient-to-r from-blue-400 to-indigo-400 bg-clip-text text-transparent", "{title}" }
                {state_badge}
            }
            div { class: "text-sm text-slate-400 font-mono", "Ontime Rust Engine v0.1.0" }
        }
    }
}
