use dioxus::prelude::*;

#[component]
pub fn Clock(elapsed_millis: u64) -> Element {
    let seconds = (elapsed_millis / 1000) % 60;
    let minutes = (elapsed_millis / 60000) % 60;
    let hours = elapsed_millis / 3600000;

    let time_str = format!("{hours:02}:{minutes:02}:{seconds:02}");

    rsx! {
        div { class: "flex flex-col items-center justify-center p-8 bg-slate-900/60 rounded-3xl border border-slate-800 backdrop-blur-xl shadow-2xl my-6",
            div { class: "text-xs font-semibold uppercase tracking-widest text-slate-400 mb-2", "Elapsed Time" }
            div { class: "text-7xl font-mono font-black tracking-tighter text-emerald-400 drop-shadow-[0_0_25px_rgba(52,211,153,0.3)]", "{time_str}" }
        }
    }
}
