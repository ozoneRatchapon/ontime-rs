//! The teleprompter (plan 008): the script, where the reader is, and the
//! shared transport. Ported from upstream ontime; the shared contract is
//! `tests/fixtures/teleprompter_*.json`, generated from upstream's own code.

pub mod script;
pub mod settings;
pub mod wrap;
