//! The playback instructions `Runtime` carries out.
//!
//! Typed and free of JSON: turning a websocket packet or an HTTP path into one
//! of these is the integration parser's job (`crate::integration`), which every
//! transport shares.

use crate::offset::OffsetMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Resume, or start whatever is loaded (loading the first event if nothing is).
    Start,
    StartNext,
    StartPrevious,
    StartId(String),
    /// Position in the rundown's events, skipped ones included, from 0. The
    /// operator counts from 1; the parser has already taken one off.
    StartIndex(usize),
    StartCue(String),
    LoadNext,
    LoadPrevious,
    LoadId(String),
    LoadIndex(usize),
    LoadCue(String),
    Pause,
    Stop,
    /// Restart the loaded event from zero.
    Reload,
    /// Milliseconds to add to the running event; negative removes time.
    AddTime(i64),
    /// Hand the rundown to the wall clock: play whatever is due now, and wait
    /// for whatever is next.
    Roll,
    /// Measure the expected times against the plan, or against the plan as
    /// shifted by when the run actually began.
    SetOffsetMode(OffsetMode),
}

impl Command {
    /// Whether a command that changes nothing is a failure the caller hears
    /// about. Upstream answers "Unable to start" when there is nothing to
    /// start, but a pause while paused is simply done.
    pub fn must_take_effect(&self) -> bool {
        matches!(
            self,
            Command::Start
                | Command::StartNext
                | Command::StartPrevious
                | Command::StartId(_)
                | Command::StartIndex(_)
                | Command::StartCue(_)
                | Command::LoadNext
                | Command::LoadPrevious
                | Command::LoadId(_)
                | Command::LoadIndex(_)
                | Command::LoadCue(_)
        )
    }
}
