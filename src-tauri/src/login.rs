//! The login flow, implemented with Tauri windows (Limusic pattern,
//! reimplemented here from the Tauri API — no Limusic code).
//!
//! - Interactive sign-in: a separate 480x720 popup opens Google's own
//!   sign-in page (`accounts.google.com/ServiceLogin?service=youtube`)
//!   with `continue=https://www.youtube.com/`. The user signs in on
//!   Google's page; when the window lands on youtube.com the app reads
//!   the webview's cookie store (`WebviewWindow::cookies()` — the full
//!   jar, HttpOnly cookies included), saves the session, and closes
//!   the popup. The popup is separate from the main UI, so this fits
//!   the no-embedded-browser rule.
//! - Silent refresh: a hidden window loads youtube.com and re-mints
//!   the cookies from the webview's own Google session, with no user
//!   interaction. Works while that session is alive; when it has
//!   genuinely expired the caller falls back to the popup.
//!
//! Only youtube.com and google.com cookies go into the `Cookie`
//! header (anything else in the jar is dropped). Cookie values are
//! never logged.
//!
//! User agent: Windows uses WebView2's default Edge UA — a Safari
//! spoof trips Google's "This browser may not be secure" check there.
//! macOS/Linux use a Safari UA instead.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use accounts::{CookieSet, Error, LoginFlow};
use tauri::webview::{PageLoadEvent, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, Url, WebviewUrl};

/// The Google sign-in page for YouTube, landing back on youtube.com.
const LOGIN_URL: &str =
    "https://accounts.google.com/ServiceLogin?service=youtube&continue=https://www.youtube.com/";
/// The page the hidden refresh window loads.
const REFRESH_URL: &str = "https://www.youtube.com/";

/// How long the sign-in popup may stay open before the flow gives up.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// How long the hidden refresh may take to load youtube.com.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);

const LOGIN_LABEL: &str = "login";
const REFRESH_LABEL: &str = "login-refresh";

/// A Safari UA for macOS/Linux, where WebView2's Edge UA does not
/// exist. Windows uses the default (see the module docs).
#[cfg(not(target_os = "windows"))]
const SAFARI_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

/// The Tauri implementation of [`LoginFlow`].
#[derive(Clone)]
pub struct TauriLoginFlow {
    app: AppHandle,
}

impl TauriLoginFlow {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn interactive_login(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>> {
        let app = self.app.clone();
        Box::pin(async move { login_popup(&app).await })
    }

    fn silent_refresh(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>> {
        let app = self.app.clone();
        Box::pin(async move { silent_refresh(&app).await })
    }
}

impl LoginFlow for TauriLoginFlow {
    fn interactive_login(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>> {
        self.interactive_login()
    }

    fn silent_refresh(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>> {
        self.silent_refresh()
    }
}

/// Open the sign-in popup and wait for a youtube.com landing with a
/// SAPISID in the jar. The jar is polled every 500 ms: after sign-in
/// the popup lands on the YouTube homepage, which is a SPA, so no
/// further full page load may ever fire — and YouTube can mint the
/// session cookies via later requests, after the landing Finished
/// event. Waiting only for the next page-load signal would leave the
/// popup sitting on the homepage until the timeout.
async fn login_popup(app: &AppHandle) -> Result<CookieSet, Error> {
    let url = Url::parse(LOGIN_URL).map_err(|e| Error::Flow(format!("bad login url: {e}")))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    // Window creation must happen on the main thread. The page-load
    // handler signals every finished load on youtube.com; the wait
    // loop below harvests after each signal AND polls the jar on a
    // 500 ms tick, returning as soon as it holds a SAPISID.
    let app2 = app.clone();
    let dispatched = app.run_on_main_thread(move || {
        destroy_window(&app2, LOGIN_LABEL);
        let builder = WebviewWindowBuilder::new(&app2, LOGIN_LABEL, WebviewUrl::External(url))
            .title("Sign in to YouTube")
            .inner_size(480.0, 720.0)
            .on_page_load(move |_window, payload| {
                if matches!(payload.event(), PageLoadEvent::Finished)
                    && payload.url().host_str() == Some("www.youtube.com")
                {
                    let _ = tx.send(());
                }
            });
        let builder = apply_user_agent(builder);
        if let Err(e) = builder.build() {
            tracing::warn!(error = %e, "could not open the sign-in window");
        }
    });
    if dispatched.is_err() {
        return Err(Error::Flow("could not dispatch the sign-in window".into()));
    }

    let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            destroy_window(app, LOGIN_LABEL);
            return Err(Error::LoginTimeout);
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                if app.get_webview_window(LOGIN_LABEL).is_none() {
                    return Err(Error::LoginCancelled);
                }
                // Poll the jar: the homepage is a SPA, so the landing
                // Finished event may be the last signal ever, while the
                // session cookies can arrive via later requests.
                match harvest_if_signed_in(app, LOGIN_LABEL).await {
                    Ok(Some(cookies)) => {
                        destroy_window(app, LOGIN_LABEL);
                        return Ok(cookies);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "could not read the sign-in cookies; waiting");
                    }
                }
            }
            signal = rx.recv() => {
                if signal.is_none() {
                    return Err(Error::LoginCancelled);
                }
                match harvest_if_signed_in(app, LOGIN_LABEL).await {
                    Ok(Some(cookies)) => {
                        destroy_window(app, LOGIN_LABEL);
                        return Ok(cookies);
                    }
                    Ok(None) => {
                        // Landed on youtube.com but not signed in yet —
                        // keep waiting for the next tick or page load.
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "could not read the sign-in cookies; waiting");
                    }
                }
            }
            _ = tokio::time::sleep(remaining) => {
                destroy_window(app, LOGIN_LABEL);
                return Err(Error::LoginTimeout);
            }
        }
    }
}

/// Load youtube.com in a hidden window and harvest the re-minted
/// cookies. The jar is polled every 500 ms until it holds a SAPISID
/// or the 30 s timeout hits — the same race as the popup applies
/// here: the Finished event fires on page load, but the refreshed
/// cookies can arrive via later requests. One attempt only — the
/// caller falls back to the popup.
async fn silent_refresh(app: &AppHandle) -> Result<CookieSet, Error> {
    let url = Url::parse(REFRESH_URL).map_err(|e| Error::Flow(format!("bad refresh url: {e}")))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    let app2 = app.clone();
    let dispatched = app.run_on_main_thread(move || {
        destroy_window(&app2, REFRESH_LABEL);
        let builder = WebviewWindowBuilder::new(&app2, REFRESH_LABEL, WebviewUrl::External(url))
            .title("YouTube")
            .visible(false)
            .skip_taskbar(true)
            .inner_size(1024.0, 768.0)
            .on_page_load(move |_window, payload| {
                if matches!(payload.event(), PageLoadEvent::Finished)
                    && payload.url().host_str() == Some("www.youtube.com")
                {
                    let _ = tx.send(());
                }
            });
        let builder = apply_user_agent(builder);
        if let Err(e) = builder.build() {
            tracing::warn!(error = %e, "could not open the refresh window");
        }
    });
    if dispatched.is_err() {
        return Err(Error::Flow("could not dispatch the refresh window".into()));
    }

    let outcome = tokio::time::timeout(REFRESH_TIMEOUT, async {
        loop {
            if app.get_webview_window(REFRESH_LABEL).is_none() {
                return Err(Error::Flow("refresh window closed".into()));
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    match harvest_if_signed_in(app, REFRESH_LABEL).await {
                        Ok(Some(cookies)) => return Ok(cookies),
                        Ok(None) => {}
                        Err(e) => return Err(e),
                    }
                }
                signal = rx.recv() => {
                    match signal {
                        Some(()) => match harvest_if_signed_in(app, REFRESH_LABEL).await {
                            Ok(Some(cookies)) => return Ok(cookies),
                            Ok(None) => {}
                            Err(e) => return Err(e),
                        },
                        None => return Err(Error::Flow("refresh window closed".into())),
                    }
                }
            }
        }
    })
    .await;
    destroy_window(app, REFRESH_LABEL);
    match outcome {
        Ok(result) => result,
        Err(_) => Err(Error::LoginTimeout),
    }
}

/// Read the window's cookies; `Ok(Some(cookies))` once the jar holds
/// a SAPISID, `Ok(None)` while it does not, `Err` when the jar cannot
/// be read at all. Cookie values are never logged.
async fn harvest_if_signed_in(app: &AppHandle, label: &str) -> Result<Option<CookieSet>, Error> {
    match read_cookies(app, label).await {
        Ok(cookies) if cookies.signed_in() => Ok(Some(cookies)),
        Ok(_) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Read the webview's cookie store and build the `Cookie` header from
/// the youtube.com and google.com cookies only (the google.com ones
/// ride along with the Google sign-in; anything else is dropped). The
/// store read must run on the main thread.
async fn read_cookies(app: &AppHandle, label: &str) -> Result<CookieSet, Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app2 = app.clone();
    let label_owned = label.to_string();
    if app
        .run_on_main_thread(move || {
            let result = app2
                .get_webview_window(&label_owned)
                .ok_or_else(|| "window gone".to_string())
                .and_then(|window| window.cookies().map_err(|e| e.to_string()));
            let _ = tx.send(result);
        })
        .is_err()
    {
        return Err(Error::Flow("could not read cookies".into()));
    }
    let cookies = rx
        .await
        .map_err(|_| Error::Flow("could not read cookies".into()))?
        .map_err(Error::Flow)?;
    let pairs = cookies
        .iter()
        .filter(|cookie| match cookie.domain() {
            None => true,
            Some(domain) => {
                let domain = domain.strip_prefix('.').unwrap_or(domain);
                domain == "youtube.com"
                    || domain.ends_with(".youtube.com")
                    || domain == "google.com"
                    || domain.ends_with(".google.com")
            }
        })
        .map(|cookie| (cookie.name().to_string(), cookie.value().to_string()));
    Ok(CookieSet::from_pairs(pairs))
}

/// Destroy a login window if it exists. Best effort; a missing window
/// is not an error.
fn destroy_window(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.destroy();
    }
}

/// Windows keeps WebView2's default UA; other platforms get a Safari
/// UA (see the module docs).
fn apply_user_agent<R: tauri::Runtime, M: tauri::Manager<R>>(
    builder: WebviewWindowBuilder<'_, R, M>,
) -> WebviewWindowBuilder<'_, R, M> {
    #[cfg(not(target_os = "windows"))]
    {
        builder.user_agent(SAFARI_UA)
    }
    #[cfg(target_os = "windows")]
    {
        builder
    }
}
