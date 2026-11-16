//! YouTube account sessions.
//!
//! The login popup (Limusic-style, implemented in `src-tauri`) mints a
//! session on the user's own machine — the session is trusted there, so
//! the app signs in the same way a browser does. This crate owns what
//! happens after the harvest:
//!
//! - [`CookieSet`]: the harvested cookies as one `Cookie` header value.
//! - [`SecretString`]: a cookie value that never prints (`[redacted]`).
//! - [`AccountStore`]: account metadata in SQLite (`accounts.db`).
//! - [`SecretStore`]: the cookie itself, in the OS keyring
//!   ([`KeyringStore`]) or, when no keyring is available, an
//!   AES-256-GCM file ([`EncryptedFileStore`], see its limits in
//!   `secrets.rs`).
//! - [`LoginFlow`]: the trait the app implements with its Tauri popup
//!   (interactive sign-in) and hidden refresh window.
//! - [`SessionManager`]: restores the active account at startup, saves
//!   fresh cookies, heals a dead session when the InnerTube client
//!   reports 401/403, and writes rotated cookies back to the secret
//!   store.
//!
//! Blocked: `validate`, `switch_account`, `start_keepalive`,
//! `list_identities`, and `select_identity` return
//! [`Error::MissingCapture`] — their request shape is not in the
//! fixtures (`docs/missing-captures.md` §1).

pub mod blocked;
pub mod cookies;
pub mod error;
pub mod flow;
pub mod manager;
pub mod secret;
pub mod secrets;
pub mod store;

pub use blocked::{AccountInfo, Identity};
pub use cookies::{parse_header, parse_netscape, CookieSet};
pub use error::Error;
pub use flow::LoginFlow;
pub use manager::{AccountStatus, SessionManager};
pub use secret::SecretString;
pub use secrets::{
    open_secret_store, EncryptedFileStore, KeyringStore, MemorySecretStore, SecretBackend,
    SecretStore,
};
pub use store::{Account, AccountStore};
