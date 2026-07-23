use ontime_core::{
    engine::RundownEngine,
    parser::RundownParser,
    types::{PlaybackState, RundownState, WsAction},
};

#[test]
fn test_default_rundown_state() {
    let state = RundownState::default();
    assert_eq!(state.playback_state, PlaybackState::Stopped);
    assert_eq!(state.events.len(), 3);
    assert!(!state.checksum.is_empty());
}

#[test]
fn test_timer_playback_actions() {
    let mut state = RundownState::default();

    RundownEngine::apply_action(&mut state, WsAction::StartTimer);
    assert_eq!(state.playback_state, PlaybackState::Running);
    assert!(state.started_at_utc.is_some());

    RundownEngine::apply_action(&mut state, WsAction::PauseTimer);
    assert_eq!(state.playback_state, PlaybackState::Paused);

    RundownEngine::apply_action(&mut state, WsAction::ResetTimer);
    assert_eq!(state.playback_state, PlaybackState::Stopped);
    assert_eq!(state.elapsed_millis, 0);
}

#[test]
fn test_event_navigation() {
    let mut state = RundownState::default();
    assert_eq!(state.current_event_index, 0);

    RundownEngine::apply_action(&mut state, WsAction::NextEvent);
    assert_eq!(state.current_event_index, 1);

    RundownEngine::apply_action(&mut state, WsAction::PreviousEvent);
    assert_eq!(state.current_event_index, 0);
}

#[test]
fn test_parse_csv_rundown() {
    let csv_data = "1, Opening Keynote, 600\n2, Product Demo, 900\n3, Break, 300";
    let events = RundownParser::parse_csv(csv_data).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].title, "Opening Keynote");
    assert_eq!(events[0].duration, 600_000);
}
