//! InnerTube transport for the YouTube web client.
//!
//! Everything here is built from the captured requests in `fixtures/`:
//! the client identity (WEB `2.20261112.10.00`, client id `1`), the
//! request headers, the minimal `context` object, and the signed-out
//! signal (`feedNudgeRenderer`, see [`is_signed_out`]).
//!
//! The browser sends a much larger `context.client` (configInfo,
//! rolloutToken, consistencyTokenJars, window sizes, ...). Those are
//! per-session browser artifacts that cannot be reproduced from a
//! captured fixture, so this crate sends the minimal context the API
//! accepts: client name/version, locale, and visitor data when known.
//! Likewise the `sec-ch-*` client-hint headers are browser-managed and
//! are deliberately not sent.
//!
//! Auth is cookie-based, exactly like the replay tool proved: the
//! `Cookie` header from the user's browser plus
//! `Authorization: SAPISIDHASH <epoch>_<sha1(epoch SAPISID origin)>`.
//! There is no OAuth flow. A login popup (separate from the main UI)
//! captures the cookies once; this crate only stores and sends them.
//!
//! # Layout
//!
//! - [`Session`] / [`SharedSession`]: the signed-in state, behind an
//!   `Arc<RwLock<..>>`, with a [`tokio::sync::Notify`] that fires when
//!   the cookie changes.
//! - [`Clients`]: client identities from the embedded `clients.json`,
//!   overridable with a file.
//! - [`context_for`]: the `context` JSON object for a request.
//! - [`headers`]: the request headers for a client + session.
//! - [`Innertube`]: `post()` with retry and session healing.
//! - [`is_signed_out`]: the signed-out signal from the captured
//!   signed-out home fixture.

mod auth;
mod client;
mod clients;
mod context;
mod error;
mod headers;
mod session;
mod signed_out;

pub use auth::sapisid_hash;
pub use client::{http_client, without_healing, HealCallback, Innertube};
pub use clients::{Client, Clients};
pub use context::context_for;
pub use error::Error;
pub use headers::headers;
pub use session::{sapisid_value, Locale, Session, SharedSession};
pub use signed_out::is_signed_out;

/// The InnerTube base URL (no trailing slash).
pub const INNERTUBE_BASE: &str = "https://www.youtube.com/youtubei/v1";
/// The origin used for SAPISIDHASH and the `x-origin` header.
pub const INNERTUBE_ORIGIN: &str = "https://www.youtube.com";
