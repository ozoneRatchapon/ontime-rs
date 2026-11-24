//! The HTTP surface, independent of any transport.
//!
//! The Cloudflare Worker — and any other host — translates its own request type
//! into [`ApiRequest`], calls [`handle`], and carries out the [`Effect`]s it gets
//! back. Keeping the routing here rather than in the Worker is what stops a second
//! host from becoming a second implementation that quietly drifts from the first.
//!
//! Split by responsibility; everything is re-exported here, so callers keep
//! using `crate::api::…`.

mod access;
mod playback;
mod project_settings;
mod routes;
mod share_url;
mod types;

pub use access::*;
pub use playback::*;
pub(crate) use project_settings::*;
pub use routes::*;
pub use share_url::*;
pub use types::*;
