//! The session manager: the single place that owns the live session.
//!
//! It restores the active account at startup, stores fresh cookies
//! (metadata in SQLite, the cookie itself in the secret store), and
//! heals a dead session when the InnerTube client reports 401/403:
//! silent refresh first, interactive popup when the webview session
//! has genuinely expired.
//!
//! It also watches the session for rotated cookies: [`SessionManager::spawn_cookie_sync`]
//! runs a background task on the session's `cookie_changed`
//! notification and writes the merged cookie back to the secret
//! store for the active account, so a rotation survives a restart.
//!
//! Blocked operations (`validate`, `switch_account`,
//! `start_keepalive`, `list_identities`, `select_identity`) are
//! stubs that return [`Error::MissingCapture`] — their request shape
//! is not in the fixtures (`docs/missing-captures.md` §1).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use innertube::{HealCallback, SharedSession};
use serde::Serialize;
use tracing::warn;

use crate::blocked::{self, AccountInfo, Identity};
use crate::cookies::{parse_header, parse_netscape};
use crate::{Account, AccountStore, CookieSet, Error, LoginFlow, SecretStore, SecretString};

/// What the UI shows about the account. The id is `ga-<hex>` — safe
/// to display; the name/handle are unknown until `validate` runs
/// (blocked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountStatus {
    pub signed_in: bool,
    pub account_id: Option<String>,
}

/// Owns the live session, the account store, the secret store, and
/// the login flow.
pub struct SessionManager {
    store: Arc<Mutex<AccountStore>>,
    secrets: Arc<dyn SecretStore>,
    session: SharedSession,
    flow: Arc<dyn LoginFlow>,
}

impl SessionManager {
    /// Build a manager around an already-created shared session.
    pub fn new(
        store: Arc<Mutex<AccountStore>>,
        secrets: Arc<dyn SecretStore>,
        session: SharedSession,
        flow: Arc<dyn LoginFlow>,
    ) -> Self {
        Self {
            store,
            secrets,
            session,
            flow,
        }
    }

    /// The live session the InnerTube client reads.
    pub fn session(&self) -> &SharedSession {
        &self.session
    }

    fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, AccountStore>, Error> {
        self.store
            .lock()
            .map_err(|e| Error::Flow(format!("account store lock poisoned: {e}")))
    }

    /// Add an account from a cookie import: either a raw `Cookie`
    /// header string or a Netscape `cookies.txt` file (detected by
    /// shape). Parses, stores (metadata in SQLite, cookie in the
    /// secret store), makes it active, and pushes it into the live
    /// session. Does NOT validate — validation is blocked (see the
    /// module docs).
    pub async fn add_account(&self, input: &str) -> Result<AccountStatus, Error> {
        let cookies = if is_netscape(input) {
            parse_netscape(input)?
        } else {
            parse_header(input)?
        };
        self.add_account_cookies(&cookies).await
    }

    /// Add an already-parsed cookie set (the popup path). Same
    /// storage steps as [`SessionManager::add_account`].
    pub async fn add_account_cookies(&self, cookies: &CookieSet) -> Result<AccountStatus, Error> {
        let id = cookies.account_id().ok_or(Error::NotSignedIn)?;
        // Secret first: if the row insert fails, the secret is merely
        // orphaned; the reverse order would leave a row with no secret.
        self.secrets.set(&id, &cookies.cookie)?;
        let account = Account {
            id: id.clone(),
            data_sync_id: cookies.data_sync_id.clone(),
            identity_json: None,
            visitor_data: cookies.visitor_data.clone(),
            added_at: Self::now_unix(),
        };
        {
            let mut store = self.lock_store()?;
            store.insert(&account)?;
            store.set_active(&id)?;
        }
        self.apply(cookies).await;
        Ok(AccountStatus {
            signed_in: true,
            account_id: Some(id),
        })
    }

    /// Restore the active account (if one was saved) into the live
    /// session. Call once at startup. Returns whether a session was
    /// restored. A dangling active pointer (row or secret missing)
    /// is cleared and counts as no session.
    pub async fn load_active(&self) -> Result<bool, Error> {
        let id = self.lock_store()?.active_id()?;
        let Some(id) = id else {
            return Ok(false);
        };
        let account = self.lock_store()?.get(&id)?;
        let Some(account) = account else {
            self.lock_store()?.clear_active()?;
            return Ok(false);
        };
        let secret = match self.secrets.get(&id)? {
            Some(secret) => secret,
            None => {
                self.lock_store()?.clear_active()?;
                return Ok(false);
            }
        };
        let cookies = CookieSet {
            cookie: secret,
            visitor_data: account.visitor_data,
            data_sync_id: account.data_sync_id,
        };
        if !cookies.signed_in() {
            return Ok(false);
        }
        self.apply(&cookies).await;
        Ok(true)
    }

    /// Sign in through the visible popup, then save the session.
    pub async fn sign_in_interactive(&self) -> Result<AccountStatus, Error> {
        let cookies = self.flow.interactive_login().await?;
        self.add_account_cookies(&cookies).await
    }

    /// Sign out: clear the live session and the active pointer.
    /// Stored accounts and their secrets are kept, so the user can
    /// come back without signing in again (once switching exists).
    pub async fn sign_out(&self) -> Result<AccountStatus, Error> {
        self.session.set_cookie(None).await;
        self.session.set_visitor_data(None).await;
        self.session.set_data_sync_id(None).await;
        self.lock_store()?.clear_active()?;
        Ok(AccountStatus {
            signed_in: false,
            account_id: None,
        })
    }

    /// Delete an account: its row and its secret. If it was active,
    /// the live session is cleared too.
    pub async fn remove_account(&self, id: &str) -> Result<(), Error> {
        let was_active = self.lock_store()?.active_id()?.as_deref() == Some(id);
        self.secrets.delete(id)?;
        self.lock_store()?.remove(id)?;
        if was_active {
            self.session.set_cookie(None).await;
            self.session.set_visitor_data(None).await;
            self.session.set_data_sync_id(None).await;
            self.lock_store()?.clear_active()?;
        }
        Ok(())
    }

    /// Every stored account, oldest first.
    pub fn list_accounts(&self) -> Result<Vec<Account>, Error> {
        self.lock_store()?.list()
    }

    /// The current account status (live session + active id).
    pub async fn status(&self) -> AccountStatus {
        let snapshot = self.session.snapshot().await;
        let account_id = self
            .lock_store()
            .ok()
            .and_then(|s| s.active_id().ok())
            .flatten();
        AccountStatus {
            signed_in: snapshot.signed_in(),
            account_id,
        }
    }

    /// The heal callback for the InnerTube client: on 401/403, try a
    /// silent refresh, fall back to the interactive popup, and save
    /// whatever comes back. On failure the session is left as-is and
    /// the client reports `SessionExpired` as before.
    ///
    /// The callback's own requests go through the webview, never
    /// through the InnerTube client, so healing cannot recurse.
    pub fn heal_callback(self: &Arc<Self>) -> HealCallback {
        let manager = Arc::clone(self);
        Arc::new(move |_session: SharedSession| {
            let manager = Arc::clone(&manager);
            Box::pin(async move {
                manager.heal_once().await;
            })
        })
    }

    async fn heal_once(&self) {
        match self.flow.silent_refresh().await {
            Ok(cookies) => {
                if let Err(e) = self.add_account_cookies(&cookies).await {
                    warn!(error = %e, "heal: silent refresh succeeded but saving failed");
                }
                return;
            }
            Err(e) => {
                warn!(error = %e, "heal: silent refresh failed, trying the sign-in popup");
            }
        }
        match self.flow.interactive_login().await {
            Ok(cookies) => {
                if let Err(e) = self.add_account_cookies(&cookies).await {
                    warn!(error = %e, "heal: popup sign-in succeeded but saving failed");
                }
            }
            Err(e) => {
                warn!(error = %e, "heal: popup sign-in failed; session stays dead");
            }
        }
    }

    /// Push a cookie set into the live session (cookie + identity
    /// fields). `set_cookie` notifies the client's `cookie_changed`
    /// waiter, so a request waiting on a heal retries with the fresh
    /// session.
    async fn apply(&self, cookies: &CookieSet) {
        self.session
            .set_cookie(Some(cookies.cookie.expose().to_string()))
            .await;
        self.session
            .set_visitor_data(cookies.visitor_data.clone())
            .await;
        self.session
            .set_data_sync_id(cookies.data_sync_id.clone())
            .await;
    }

    /// Spawn the background task that writes rotated cookies back to
    /// the secret store. It wakes on the session's `cookie_changed`
    /// notification (a 60-second tick backstops a notification missed
    /// between loop iterations) and persists the merged cookie for
    /// the active account whenever it changed. Runs until the runtime
    /// drops it (app exit); it never logs a cookie value.
    pub fn spawn_cookie_sync(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut last_synced: Option<SecretString> = None;
            loop {
                tokio::select! {
                    _ = manager.session.cookie_changed.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                }
                if let Err(e) = manager.sync_active_cookie(&mut last_synced).await {
                    warn!(error = %e, "could not sync the rotated cookie to the secret store");
                }
            }
        })
    }

    /// One sync step, for [`SessionManager::spawn_cookie_sync`] and
    /// for tests: write the live session's cookie to the secret store
    /// for the active account, when it differs from `last_synced`.
    /// A missing active account or an empty session is a no-op.
    pub async fn sync_active_cookie(
        &self,
        last_synced: &mut Option<SecretString>,
    ) -> Result<(), Error> {
        let active = self.lock_store()?.active_id()?;
        let Some(id) = active else {
            return Ok(());
        };
        let Some(cookie) = self.session.cookie().await else {
            return Ok(());
        };
        if last_synced.as_ref().map(|s| s.expose()) == Some(cookie.as_str()) {
            return Ok(());
        }
        let secret = SecretString::new(cookie);
        self.secrets.set(&id, &secret)?;
        *last_synced = Some(secret);
        Ok(())
    }

    // -- Blocked operations (docs/missing-captures.md §1) -----------

    /// Validate a cookie set against the account-menu request and
    /// return the signed-in account. BLOCKED: the account-menu
    /// request/response shape is not in the fixtures.
    pub async fn validate(&self, _cookies: &CookieSet) -> Result<AccountInfo, Error> {
        Err(blocked::missing_capture(
            "validate(cookie) — needs the account-menu capture (docs/missing-captures.md §1)",
        ))
    }

    /// Switch to a stored account, validating it first. BLOCKED: it
    /// validates, so it needs the account-menu capture
    /// (docs/missing-captures.md §1). Refuses without touching the
    /// active account.
    pub async fn switch_account(&self, _id: &str) -> Result<AccountStatus, Error> {
        Err(blocked::missing_capture(
            "switch_account(id) — validates first, so it needs the account-menu capture (docs/missing-captures.md §1)",
        ))
    }

    /// Keep the session alive by calling the account menu on an
    /// interval, so cookies keep rotating. BLOCKED: it calls the
    /// account menu, whose shape is not in the fixtures
    /// (docs/missing-captures.md §1).
    pub fn start_keepalive(&self, _interval: Duration) -> Result<(), Error> {
        Err(blocked::missing_capture(
            "start_keepalive — needs the account-menu capture (docs/missing-captures.md §1)",
        ))
    }

    /// List the brand accounts under the signed-in Google account.
    /// BLOCKED: needs the account-identities capture
    /// (docs/missing-captures.md §1).
    pub async fn list_identities(&self) -> Result<Vec<Identity>, Error> {
        Err(blocked::missing_capture(
            "list_identities — needs the account-identities capture (docs/missing-captures.md §1)",
        ))
    }

    /// Switch to one brand identity. BLOCKED: needs the
    /// account-select capture (docs/missing-captures.md §1).
    pub async fn select_identity(&self, _data_sync_id: &str) -> Result<AccountStatus, Error> {
        Err(blocked::missing_capture(
            "select_identity — needs the account-select capture (docs/missing-captures.md §1)",
        ))
    }
}

/// A Netscape export is tab-separated lines with 7 fields (or the
/// `# Netscape HTTP Cookie File` header); a raw `Cookie` header is
/// `name=value` pairs separated by `;` and has no tabs.
fn is_netscape(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line.starts_with("# Netscape") || line.split('\t').count() >= 7
    })
}
