mod components;

use components::{
    clock::Clock, control_panel::ControlPanel, event_list::EventList, header::Header,
    importer::RundownImporter,
};
use dioxus::prelude::*;
use ontime_core::{
    engine::RundownEngine,
    types::{EventItem, RundownState, WsAction},
};

fn app() -> Element {
    let mut state = use_signal(RundownState::default);

    let handle_action = move |action: WsAction| {
        let mut current_state = state.write();
        RundownEngine::apply_action(&mut current_state, action);
    };

    let handle_import = move |events: Vec<EventItem>| {
        let mut current_state = state.write();
        current_state.events = events;
        current_state.current_event_index = 0;
        current_state.elapsed_millis = 0;
        current_state.checksum = current_state.compute_checksum();
    };

    let rundown = state.read();

    rsx! {
        div { class: "min-h-screen bg-slate-950 text-slate-100 font-sans selection:bg-indigo-500 selection:text-white",
            Header {
                title: rundown.project.title.clone(),
                playback_state: rundown.playback_state.clone(),
            }
            main { class: "container mx-auto px-4 py-8 max-w-5xl",
                Clock { elapsed_millis: rundown.elapsed_millis }
                ControlPanel {
                    playback_state: rundown.playback_state.clone(),
                    on_action: handle_action,
                }
                RundownImporter { on_import: handle_import }
                EventList {
                    events: rundown.events.clone(),
                    current_index: rundown.current_event_index,
                }
            }
        }
    }
}

fn main() {
    dioxus::launch(app);
}
