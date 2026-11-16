//! The signed-in session: cookie, visitor data, delegated identity,
//! locale. The data lives behind an `Arc<RwLock<..>>` ([`SharedSession`])
//! so the transport, the login popup, and the healer can all share it,
//! and a [`tokio::sync::Notify`] fires whenever the cookie changes so
//! a 401-heal can wait for a fresh cookie instead of polling.
//!
//! Cookie rules, all from the replay work in Phase 0:
//! - The cookie is a `Cookie` header string (`NAME=value; NAME=value`).
//! - The auth identity is the `SAPISID` value, falling back to
//!   `__Secure-3PAPISID` (same preference as the replay tool).
//! - Merging `Set-Cookie` headers: a pair replaces an existing pair
//!   with the same name, otherwise it is appended. Entries with an
//!   empty value (`NAME=; ...`) are deletions and are IGNORED —
//!   applying them would sign the user out from under the app.

use std::sync::Arc;

use tokio::sync::{Notify, RwLock};

/// Locale for the `context.client` object: `hl` is the interface
/// language (`en` in the capture), `gl` the content region (`ID`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locale {
    /// Interface language, e.g. `en`.
    pub hl: String,
    /// Content region, e.g. `ID`.
    pub gl: String,
}

impl Default for Locale {
    /// The locale from the captured requests.
    fn default() -> Self {
        Self {
            hl: "en".to_string(),
            gl: "ID".to_string(),
        }
    }
}

/// The session data. Plain data; sharing and change notification live
/// in [`SharedSession`].
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// The full `Cookie` header value, or `None` when signed out.
    pub cookie: Option<String>,
    /// `visitorData` for the `context.client` object, when known.
    pub visitor_data: Option<String>,
    /// Delegated identity for `context.user.onBehalfOfUser`, when known.
    pub data_sync_id: Option<String>,
    /// Locale for `context.client.gl` / `hl`.
    pub locale: Locale,
}

impl Session {
    /// Signed in means a cookie is stored that carries a SAPISID (or
    /// its `__Secure-3PAPISID` fallback) — the value SAPISIDHASH is
    /// computed from.
    pub fn signed_in(&self) -> bool {
        self.cookie.as_deref().and_then(sapisid_value).is_some()
    }

    /// The SAPISID value for SAPISIDHASH, if the session has one.
    pub fn sapisid(&self) -> Option<String> {
        self.cookie.as_deref().and_then(sapisid_value)
    }
}

/// The shared session: the data behind an `Arc<RwLock<..>>`, plus a
/// [`Notify`] that fires when the cookie value changes.
#[derive(Debug, Clone)]
pub struct SharedSession {
    inner: Arc<RwLock<Session>>,
    /// Fires (once per change) when [`SharedSession::set_cookie`] or
    /// [`SharedSession::merge_set_cookie`] changes the cookie.
    pub cookie_changed: Arc<Notify>,
}

impl Default for SharedSession {
    fn default() -> Self {
        Self::new(Session::default())
    }
}

impl SharedSession {
    /// Wrap a session.
    pub fn new(session: Session) -> Self {
        Self {
            inner: Arc::new(RwLock::new(session)),
            cookie_changed: Arc::new(Notify::new()),
        }
    }

    /// A read snapshot of the session data.
    pub async fn snapshot(&self) -> Session {
        self.inner.read().await.clone()
    }

    /// The stored cookie, if any.
    pub async fn cookie(&self) -> Option<String> {
        self.inner.read().await.cookie.clone()
    }

    /// Replace the cookie; notifies [`Self::cookie_changed`] only when
    /// the value actually changes.
    pub async fn set_cookie(&self, cookie: Option<String>) {
        let mut guard = self.inner.write().await;
        if guard.cookie != cookie {
            guard.cookie = cookie;
            self.cookie_changed.notify_waiters();
        }
    }

    /// Replace the visitor data (no notification: it is not auth).
    pub async fn set_visitor_data(&self, visitor_data: Option<String>) {
        self.inner.write().await.visitor_data = visitor_data;
    }

    /// Replace the delegated identity (no notification).
    pub async fn set_data_sync_id(&self, data_sync_id: Option<String>) {
        self.inner.write().await.data_sync_id = data_sync_id;
    }

    /// Merge `Set-Cookie` header values into the stored cookie, per
    /// [`merge_set_cookie`]. Notifies only when the cookie changes.
    /// Does nothing when signed out (there is no cookie to merge into).
    pub async fn merge_set_cookie(&self, set_cookie: &[String]) {
        let mut guard = self.inner.write().await;
        let Some(current) = guard.cookie.clone() else {
            return;
        };
        let (merged, changed) = merge_set_cookie(&current, set_cookie);
        if changed {
            guard.cookie = Some(merged);
            self.cookie_changed.notify_waiters();
        }
    }
}

/// Extract the value of one cookie (`NAME=value`) from a `Cookie`
/// header string. Returns `None` when the name is absent.
pub fn cookie_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        if k.trim() == name {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}

/// The SAPISID value from a `Cookie` header: `SAPISID` first,
/// `__Secure-3PAPISID` as a fallback (same preference as the replay
/// tool). `None` when neither is present.
pub fn sapisid_value(cookie: &str) -> Option<String> {
    cookie_value(cookie, "SAPISID").or_else(|| cookie_value(cookie, "__Secure-3PAPISID"))
}

/// Merge `Set-Cookie` header values into an existing `Cookie` header.
/// Each header contributes its first `NAME=value` pair; a pair with
/// the same name replaces the stored one, otherwise it is appended.
/// Entries with an empty value are deletions (`NAME=; ...`) and are
/// ignored. Returns the merged header and whether it changed.
pub fn merge_set_cookie(current: &str, set_cookie: &[String]) -> (String, bool) {
    let mut pairs: Vec<(String, String)> = current
        .split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return None;
            }
            let (k, v) = pair.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    let mut changed = false;
    for header in set_cookie {
        let Some(first) = header.split(';').next() else {
            continue;
        };
        let Some((name, value)) = first.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            // Deletion (`NAME=; ...`) or malformed: ignore.
            continue;
        }
        match pairs.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => {
                if *v != value {
                    *v = value.to_string();
                    changed = true;
                }
            }
            None => {
                pairs.push((name.to_string(), value.to_string()));
                changed = true;
            }
        }
    }
    let merged = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    (merged, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sapisid_prefers_sapisid_over_secure_3papisid() {
        let cookie = "SAPISID=abc; __Secure-3PAPISID=xyz; SID=1";
        assert_eq!(sapisid_value(cookie), Some("abc".to_string()));
    }

    #[test]
    fn sapisid_falls_back_to_secure_3papisid() {
        let cookie = "__Secure-3PAPISID=xyz; SID=1";
        assert_eq!(sapisid_value(cookie), Some("xyz".to_string()));
    }

    #[test]
    fn merge_replaces_appends_and_ignores_deletions() {
        let current = "SAPISID=old; SID=1";
        let set = [
            "SAPISID=new; Path=/; Secure".to_string(),
            "VISITOR=abc; Path=/".to_string(),
            "SID=; Expires=Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
        ];
        let (merged, changed) = merge_set_cookie(current, &set);
        assert!(changed);
        assert_eq!(merged, "SAPISID=new; SID=1; VISITOR=abc");
    }

    #[test]
    fn merge_noop_when_nothing_changes() {
        let current = "SAPISID=abc; SID=1";
        let set = ["SAPISID=abc; Path=/".to_string()];
        let (merged, changed) = merge_set_cookie(current, &set);
        assert!(!changed);
        assert_eq!(merged, current);
    }

    #[test]
    fn merge_ignores_a_deletion_only_response() {
        let current = "SAPISID=abc; SID=1";
        let set = ["SID=; Expires=Thu, 01 Jan 1970 00:00:00 GMT".to_string()];
        let (merged, changed) = merge_set_cookie(current, &set);
        assert!(!changed);
        assert_eq!(merged, current);
    }
}
