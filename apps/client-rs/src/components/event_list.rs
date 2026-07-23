use dioxus::prelude::*;
use ontime_core::types::EventItem;

#[component]
pub fn EventList(events: Vec<EventItem>, current_index: usize) -> Element {
    rsx! {
        div { class: "flex flex-col space-y-3 w-full max-w-4xl mx-auto p-4",
            h2 { class: "text-lg font-bold text-slate-300 mb-2 border-b border-slate-800 pb-2", "Rundown Cues" }
            for (idx , event) in events.into_iter().enumerate() {
                {
                    let is_active = idx == current_index;
                    let row_class = if is_active {
                        "flex items-center justify-between p-4 bg-indigo-950/60 border-l-4 border-indigo-500 rounded-xl shadow-lg transition-all"
                    } else {
                        "flex items-center justify-between p-4 bg-slate-900/40 border border-slate-800/80 rounded-xl hover:bg-slate-800/40 transition-all"
                    };
                    let duration_sec = event.duration / 1000;
                    let mins = duration_sec / 60;
                    let secs = duration_sec % 60;

                    rsx! {
                        div { key: "{event.id}", class: "{row_class}",
                            div { class: "flex items-center space-x-4",
                                span { class: "font-mono font-bold text-sm text-indigo-400 bg-indigo-950/80 px-2.5 py-1 rounded-md border border-indigo-800/50", "{event.cue}" }
                                div {
                                    div { class: "font-semibold text-white text-base", "{event.title}" }
                                    if let Some(sub) = event.subtitle {
                                        div { class: "text-xs text-slate-400 mt-0.5", "{sub}" }
                                    }
                                }
                            }
                            div { class: "font-mono text-sm font-semibold text-slate-300 bg-slate-800/50 px-3 py-1 rounded-lg border border-slate-700/50",
                                "{mins:02}:{secs:02}"
                            }
                        }
                    }
                }
            }
        }
    }
}
