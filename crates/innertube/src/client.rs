//! The InnerTube client: `post()` with retry and session healing.
//!
//! - URL: `{base}/{path}?prettyPrint=false` (`&` when the path
//!   already has a query).
//! - Headers are rebuilt on every attempt from a fresh session
//!   snapshot, so a refreshed cookie is never missed.
//! - Transport retry: at most 3 attempts, only on connect or timeout
//!   errors, with 500 ms doubling backoff (500 ms, 1 s).
//! - After a 2xx, any `Set-Cookie` headers are merged into the
//!   session cookie (deletions ignored) and `cookie_changed` fires
//!   when the cookie actually changes.
//! - On 401/403 while signed in: one heal is emitted through the
//!   registered callback, the caller waits for `cookie_changed` (up
//!   to the heal timeout, default 90 s), retries once, and returns
//!   [`Error::SessionExpired`] when the heal did not produce a fresh
//!   cookie. Only one heal runs at a time; concurrent 401s wait for
//!   the same heal instead of emitting another.
//! - [`without_healing`] runs a future with healing disabled, so the
//!   healer's own requests never wait on themselves.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{self, HeaderMap};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use crate::headers::headers;
use crate::{Clients, Error, SharedSession, INNERTUBE_BASE};

/// The heal callback: called once when a 401/403 arrives while signed
/// in. It receives the shared session and is expected to re-mint the
/// cookie (for example by loading the service in the login webview)
/// and store it with [`SharedSession::set_cookie`], which notifies
/// `cookie_changed`. It must not call [`Innertube::post`] with
/// healing enabled — use [`without_healing`] for its own requests.
pub type HealCallback =
    Arc<dyn Fn(SharedSession) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

tokio::task_local! {
    static WITHOUT_HEALING: ();
}

/// Run `f` with session healing disabled. The healer's own requests
/// must run inside this, or a 401 inside the heal would wait on the
/// heal it is performing.
pub async fn without_healing<F, T>(f: F) -> T
where
    F: Future<Output = T>,
{
    WITHOUT_HEALING.scope((), f).await
}

fn healing_allowed() -> bool {
    // Err when the task-local is not set (or when called outside a
    // task): healing is allowed in both cases.
    WITHOUT_HEALING.try_with(|_| ()).is_err()
}

/// Build an HTTP client that trusts the OS certificate store in
/// addition to the built-in web roots. rustls trusts only the
/// webpki roots by default, so on a machine behind a
/// TLS-inspecting proxy (or with a custom CA installed) every
/// request fails with `InvalidCertificate(UnknownIssuer)` even
/// though the system (curl, browsers) trusts the chain. Loading
/// the native roots makes the app behave like the system does.
/// A failure to read the native store only logs a warning — the
/// client still works with the built-in roots.
pub fn http_client(timeout: Duration) -> Result<reqwest::Client, Error> {
    let mut builder = reqwest::Client::builder().timeout(timeout);
    let loaded = rustls_native_certs::load_native_certs();
    if !loaded.errors.is_empty() {
        tracing::warn!(errors = ?loaded.errors, "could not load some native certificates");
    }
    for cert in loaded.certs {
        if let Ok(cert) = reqwest::Certificate::from_der(cert.as_ref()) {
            builder = builder.add_root_certificate(cert);
        }
    }
    Ok(builder.build()?)
}

/// The InnerTube client.
pub struct Innertube {
    base: String,
    http: reqwest::Client,
    clients: Clients,
    session: SharedSession,
    heal_callback: RwLock<Option<HealCallback>>,
    /// True while a heal is in flight.
    healing: Mutex<bool>,
    /// How long to wait for a fresh cookie after emitting a heal.
    heal_timeout: Duration,
    /// Base backoff between transport retries (doubled each time).
    retry_backoff: Duration,
    /// Max transport attempts per call.
    max_attempts: usize,
}

impl Innertube {
    /// Build a client with defaults: base [`INNERTUBE_BASE`], 30 s
    /// request timeout, 90 s heal timeout, 500 ms retry backoff,
    /// 3 transport attempts, no heal callback.
    pub fn new(clients: Clients, session: SharedSession) -> Result<Self, Error> {
        Self::with_timeouts(
            clients,
            session,
            Duration::from_secs(30),
            Duration::from_secs(90),
        )
    }

    /// Build with explicit timeouts (the tests use short ones).
    pub fn with_timeouts(
        clients: Clients,
        session: SharedSession,
        request_timeout: Duration,
        heal_timeout: Duration,
    ) -> Result<Self, Error> {
        let http = http_client(request_timeout)?;
        Ok(Self {
            base: INNERTUBE_BASE.to_string(),
            http,
            clients,
            session,
            heal_callback: RwLock::new(None),
            healing: Mutex::new(false),
            heal_timeout,
            retry_backoff: Duration::from_millis(500),
            max_attempts: 3,
        })
    }

    /// The shared session this client reads and writes.
    pub fn session(&self) -> &SharedSession {
        &self.session
    }

    /// Build the `context` object for the named client with the
    /// current session (see [`crate::context_for`]). Feed functions
    /// use this to build their request bodies.
    pub async fn context(&self, client: &str) -> Result<Value, Error> {
        let client = self
            .clients
            .get(client)
            .ok_or_else(|| Error::UnknownClient(client.to_string()))?;
        let session = self.session.snapshot().await;
        Ok(crate::context_for(client, &session))
    }

    /// Override the base URL (tests point it at a local mock server).
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    /// Register the heal callback (see [`HealCallback`]).
    pub async fn with_heal_callback(self, callback: HealCallback) -> Self {
        *self.heal_callback.write().await = Some(callback);
        self
    }

    /// Base backoff between transport retries (default 500 ms).
    pub fn with_retry_backoff(mut self, backoff: Duration) -> Self {
        self.retry_backoff = backoff;
        self
    }

    /// Max transport attempts per call (default 3).
    pub fn with_max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = max_attempts.max(1);
        self
    }

    /// POST `body` to `{base}/{path}?prettyPrint=false` as the named
    /// client (e.g. `"WEB"`). The caller builds the body, including
    /// its `context` (see [`crate::context_for`]). Returns the parsed
    /// JSON response. See the module docs for retry and heal rules.
    pub async fn post(&self, path: &str, client: &str, body: Value) -> Result<Value, Error> {
        let client = self
            .clients
            .get(client)
            .ok_or_else(|| Error::UnknownClient(client.to_string()))?;
        let url = self.url(path);

        let mut transport_attempts = 0usize;
        let mut healed = false;
        loop {
            let snapshot = self.session.snapshot().await;
            let headers = headers(client, &snapshot);
            let result = self
                .http
                .post(&url)
                .headers(headers)
                .json(&body)
                .send()
                .await;
            match result {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        self.merge_set_cookie(response.headers()).await;
                        let bytes = response.bytes().await?;
                        let value: Value = serde_json::from_slice(&bytes)?;
                        return Ok(value);
                    }
                    if (status.as_u16() == 401 || status.as_u16() == 403)
                        && snapshot.signed_in()
                        && healing_allowed()
                        && !healed
                    {
                        let pre_cookie = snapshot.cookie.clone();
                        match self.heal_once(&pre_cookie).await {
                            Ok(true) => {
                                // Fresh cookie: retry once. The heal
                                // retry does not consume a transport
                                // attempt.
                                healed = true;
                                continue;
                            }
                            Ok(false) | Err(_) => return Err(Error::SessionExpired),
                        }
                    }
                    if status.as_u16() == 401 || status.as_u16() == 403 {
                        if snapshot.signed_in() {
                            return Err(Error::SessionExpired);
                        }
                        return Err(Error::HttpStatus(status.as_u16()));
                    }
                    return Err(Error::HttpStatus(status.as_u16()));
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && transport_attempts + 1 < self.max_attempts {
                        let backoff = self.retry_backoff * (1u32 << transport_attempts);
                        transport_attempts += 1;
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    return Err(Error::Http(err));
                }
            }
        }
    }

    fn url(&self, path: &str) -> String {
        let path = path.strip_prefix('/').unwrap_or(path);
        let sep = if path.contains('?') { '&' } else { '?' };
        format!("{}/{path}{sep}prettyPrint=false", self.base)
    }

    async fn merge_set_cookie(&self, headers: &HeaderMap) {
        let set_cookie: Vec<String> = headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        if !set_cookie.is_empty() {
            self.session.merge_set_cookie(&set_cookie).await;
        }
    }

    /// Emit one heal (or wait for the heal another task emitted) and
    /// report whether the cookie changed. `Ok(false)` means the heal
    /// ran but produced no fresh cookie; `Err(SessionExpired)` means
    /// the wait timed out.
    async fn heal_once(&self, pre_cookie: &Option<String>) -> Result<bool, Error> {
        // Claim the heal: only one task calls the callback.
        let mine = {
            let mut healing = self.healing.lock().await;
            if *healing {
                false
            } else {
                *healing = true;
                true
            }
        };
        // Register the waiter BEFORE the callback runs, or the
        // notification can be missed.
        let notified = self.session.cookie_changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if mine {
            let callback = self.heal_callback.read().await.clone();
            match callback {
                Some(callback) => {
                    callback(self.session.clone()).await;
                }
                None => {
                    // Nobody can heal: release the claim and report
                    // no fresh cookie without waiting.
                    *self.healing.lock().await = false;
                    return Ok(false);
                }
            }
            *self.healing.lock().await = false;
        }
        match tokio::time::timeout(self.heal_timeout, notified).await {
            Ok(()) => Ok(self.session.cookie().await != *pre_cookie),
            Err(_) => Err(Error::SessionExpired),
        }
    }
}
