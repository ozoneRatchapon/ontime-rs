//! The teleprompter (plan 008): the script, where the reader is, and the
//! shared transport. Ported from upstream ontime; the shared contract is
//! `tests/fixtures/teleprompter_*.json`, generated from upstream's own code.

pub mod position;
pub mod room;
pub(crate) mod routes;
pub mod script;
pub mod settings;
pub mod transport;
pub mod wrap;
