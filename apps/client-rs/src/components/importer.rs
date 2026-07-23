use dioxus::prelude::*;
use ontime_core::{parser::RundownParser, types::EventItem};

#[component]
pub fn RundownImporter(on_import: EventHandler<Vec<EventItem>>) -> Element {
    let mut raw_csv = use_signal(String::new);
    let mut parse_error = use_signal(|| Option::<String>::None);

    let handle_parse = move |_| {
        let text = raw_csv.read();
        match RundownParser::parse_csv(&text) {
            Ok(events) => {
                if events.is_empty() {
                    parse_error.set(Some("No valid events found in CSV.".to_string()));
                } else {
                    parse_error.set(None);
                    on_import.call(events);
                }
            }
            Err(err) => {
                parse_error.set(Some(format!("Parse error: {err}")));
            }
        }
    };

    rsx! {
        div { class: "p-6 bg-slate-900/60 rounded-2xl border border-slate-800 backdrop-blur-xl shadow-xl my-6 max-w-4xl mx-auto",
            h3 { class: "text-lg font-bold text-slate-200 mb-3 flex items-center space-x-2",
                span { "📥 Import Rundown (CSV)" }
            }
            textarea {
                class: "w-full h-28 p-3 bg-slate-950/80 border border-slate-800 rounded-xl text-slate-200 font-mono text-sm focus:outline-none focus:border-indigo-500 transition-all mb-3",
                placeholder: "Enter CSV format (e.g. 1, Intro, 300\\n2, Keynote, 1800)...",
                value: "{raw_csv}",
                oninput: move |evt| raw_csv.set(evt.value())
            }
            if let Some(err) = parse_error.read().as_ref() {
                div { class: "text-xs font-semibold text-rose-400 bg-rose-950/40 p-2.5 rounded-lg border border-rose-800/50 mb-3", "{err}" }
            }
            button {
                class: "px-5 py-2 bg-indigo-600 hover:bg-indigo-500 active:scale-95 text-white font-semibold text-sm rounded-xl border border-indigo-500 transition-all shadow-md",
                onclick: handle_parse,
                "Import Events"
            }
        }
    }
}
