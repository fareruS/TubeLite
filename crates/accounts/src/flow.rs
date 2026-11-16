//! The login flow: what the app must do to mint a fresh session.
//! The app (Tauri popup + hidden refresh window) implements this;
//! the [`SessionManager`](crate::SessionManager) calls it when a
//! session is missing or dead. The crate never touches a webview
//! itself, so it stays testable without Tauri.

use std::future::Future;
use std::pin::Pin;

use crate::{CookieSet, Error};

/// Mint a fresh session. Both methods resolve with the harvested
/// cookies, or fail (see [`Error`]) when the user cancels, the window
/// times out, or the jar has no SAPISID.
pub trait LoginFlow: Send + Sync {
    /// Open the visible sign-in popup (480x720) and wait for the user
    /// to sign in on Google's own page.
    fn interactive_login(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>>;

    /// Silently re-mint cookies in a hidden webview, with no user
    /// interaction. Works while the webview's Google session is still
    /// alive; fails when it has genuinely expired, and the caller
    /// falls back to [`LoginFlow::interactive_login`].
    fn silent_refresh(&self)
        -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>>;
}
