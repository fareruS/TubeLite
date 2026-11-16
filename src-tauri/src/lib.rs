//! App crate — the only place that knows about all other crates.
//! IPC is the only channel between UI and core.

pub mod guard;
pub mod logging;
pub mod login;
pub mod player_ipc;
pub mod source;

use accounts::{open_secret_store, AccountStatus, AccountStore, SecretBackend, SessionManager};
use feeds::{FeedPage, Feeds};
use innertube::{Clients, Innertube, Session, SharedSession};
use login::TauriLoginFlow;
use source::AnySource;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State, WebviewWindow};
use yt_model::{
    Capabilities, ChannelId, ChannelPage, Page, PlayableMedia, PlaylistId, QualityPref, VideoCard,
    VideoDetails, VideoId,
};
use yt_source::{
    traits::{SearchKind, SearchQuery},
    StreamSource,
};

pub struct AppState {
    pub store: Mutex<store::Store>,
    pub source: AnySource,
    pub player: player_ipc::PlayerSlot,
    /// The account session manager (login popup + heal flow).
    pub accounts: Arc<SessionManager>,
    /// Where the cookies live ("OS keyring" or "encrypted file").
    /// The UI shows a warning when the fallback is in use.
    pub secret_backend: SecretBackend,
    /// The background task that writes rotated cookies back to the
    /// secret store. Held so it is not dropped at startup.
    pub cookie_sync: tauri::async_runtime::JoinHandle<()>,
    /// The live feeds client over the new transport, if it built.
    /// `None` only when the embedded client data is bad — startup
    /// must not fail over a feed client.
    pub feeds: Option<Arc<Feeds>>,
}

fn check_window(window: &WebviewWindow) -> Result<(), String> {
    if guard::is_approved_window(window.label()) {
        Ok(())
    } else {
        Err(format!(
            "IPC rejected: unapproved webview '{}'",
            window.label()
        ))
    }
}

#[tauri::command]
async fn search_videos(
    window: WebviewWindow,
    state: State<'_, AppState>,
    query: String,
) -> Result<Page<VideoCard>, String> {
    check_window(&window)?;
    state
        .source
        .search(&SearchQuery {
            text: query,
            kind: SearchKind::All,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_video(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<VideoDetails, String> {
    check_window(&window)?;
    state
        .source
        .video(&VideoId(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_channel(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<ChannelPage, String> {
    check_window(&window)?;
    state
        .source
        .channel(&ChannelId(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_playlist(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
) -> Result<Page<VideoCard>, String> {
    check_window(&window)?;
    state
        .source
        .playlist(&PlaylistId(id), None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_playable(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
    height: Option<u32>,
) -> Result<PlayableMedia, String> {
    check_window(&window)?;
    let pref = match height {
        None => QualityPref::AudioOnly,
        Some(0) => QualityPref::Auto,
        Some(h) => QualityPref::Height(h),
    };
    state
        .source
        .playable(&VideoId(id), pref)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_capabilities(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Capabilities, String> {
    check_window(&window)?;
    Ok(state.source.capabilities())
}

/// Phase 0 spike hook: the UI reports the video rectangle (physical px =
/// display scale × page zoom) over IPC so mpv's child window can be placed
/// beneath the transparent webview. On this Linux VM there is no child
/// window yet — the rect is logged (redacted: numbers only) and stored for
/// the spike. Windows implementation lands in Phase 0.
#[tauri::command]
fn report_video_rect(
    window: WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    check_window(&window)?;
    tracing::info!(x, y, width, height, "video rect reported");
    Ok(())
}

#[tauri::command]
fn get_setting(state: State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store.set_setting(&key, &value).map_err(|e| e.to_string())
}

/// Sign in through the popup (480x720 Google sign-in page). Resolves
/// with the account status once the session is saved.
#[tauri::command]
async fn sign_in(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<AccountStatus, String> {
    check_window(&window)?;
    state
        .accounts
        .sign_in_interactive()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn sign_out(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<AccountStatus, String> {
    check_window(&window)?;
    state.accounts.sign_out().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn account_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<AccountStatus, String> {
    check_window(&window)?;
    Ok(state.accounts.status().await)
}

/// Import a cookie export (the manual fallback when the popup cannot
/// mint a session): either a raw `Cookie` header or a Netscape
/// `cookies.txt` file. `path` is a file path on the user's machine,
/// chosen in the UI.
#[tauri::command]
async fn import_cookies(
    window: WebviewWindow,
    state: State<'_, AppState>,
    path: String,
) -> Result<AccountStatus, String> {
    check_window(&window)?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    state
        .accounts
        .add_account(&text)
        .await
        .map_err(|e| e.to_string())
}

/// Where the cookies are stored ("OS keyring" or "encrypted file").
/// The UI warns when the fallback is in use.
#[tauri::command]
fn secret_backend(window: WebviewWindow, state: State<'_, AppState>) -> Result<String, String> {
    check_window(&window)?;
    Ok(match state.secret_backend {
        SecretBackend::Keyring => "OS keyring".to_string(),
        SecretBackend::EncryptedFile => "encrypted file".to_string(),
    })
}

/// The signed-in home feed over the new transport — the proof that
/// the popup's session actually reaches YouTube. The Home/Search
/// pages still run on the old source chain; switching them over is
/// the next step.
#[tauri::command]
async fn get_home_feed(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<FeedPage, String> {
    check_window(&window)?;
    let Some(feeds) = &state.feeds else {
        return Err("live feeds unavailable: the InnerTube client did not build".into());
    };
    feeds.home().await.map_err(|e| e.to_string())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
fn list_subscriptions(state: State<'_, AppState>) -> Result<Vec<store::Subscription>, String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store.list_subscriptions().map_err(|e| e.to_string())
}

#[tauri::command]
fn subscribe(state: State<'_, AppState>, channel_id: String, title: String) -> Result<(), String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store
        .add_subscription(&channel_id, &title, now_unix())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn unsubscribe(state: State<'_, AppState>, channel_id: String) -> Result<(), String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store
        .remove_subscription(&channel_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_watch_later(state: State<'_, AppState>) -> Result<Vec<store::WatchLaterEntry>, String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store.list_watch_later().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_watch_later(
    state: State<'_, AppState>,
    video_id: String,
    title: String,
    channel: String,
) -> Result<(), String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store
        .add_watch_later(&video_id, &title, &channel, now_unix())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_watch_later(state: State<'_, AppState>, video_id: String) -> Result<(), String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store
        .remove_watch_later(&video_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_history(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<store::HistoryEntry>, String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store
        .list_history(limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn record_history_entry(
    state: State<'_, AppState>,
    video_id: String,
    title: String,
    channel: String,
    position_secs: f64,
    duration_secs: Option<f64>,
) -> Result<(), String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store
        .record_history(
            &video_id,
            &title,
            &channel,
            now_unix(),
            position_secs,
            duration_secs,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_resume_position(
    state: State<'_, AppState>,
    video_id: String,
    position_secs: f64,
) -> Result<(), String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store
        .save_resume(&video_id, position_secs, now_unix())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_resume_position(
    state: State<'_, AppState>,
    video_id: String,
) -> Result<Option<f64>, String> {
    let store = state
        .store
        .lock()
        .map_err(|e| format!("store lock poisoned: {e}"))?;
    store.get_resume(&video_id).map_err(|e| e.to_string())
}

/// The on-disk store file inside the OS app-data directory
/// (`~/.local/share/com.tubelite.desktop/` on Linux,
/// `%APPDATA%/com.tubelite.desktop/` on Windows).
pub fn store_path(app_data_dir: &std::path::Path) -> std::path::PathBuf {
    app_data_dir.join("tubelite.db")
}

/// Open the persistent store, creating the app-data directory first.
/// A failed open must fail startup loudly — never silently fall back to
/// an in-memory store and lose the user's settings/history.
fn open_app_store(app_data_dir: &std::path::Path) -> Result<store::Store, String> {
    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| format!("cannot create app-data dir {}: {e}", app_data_dir.display()))?;
    let path = store_path(app_data_dir);
    store::Store::open(&path).map_err(|e| format!("cannot open store {}: {e}", path.display()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .setup(|app| {
            // The store needs the OS app-data dir, which only exists once
            // the app handle does — so it (and everything built from its
            // settings) is created here, not before the builder.
            let app_data_dir = app
                .handle()
                .path()
                .app_data_dir()
                .map_err(|e| format!("cannot resolve app-data dir: {e}"))?;
            let app_store = open_app_store(&app_data_dir)?;

            // Source chain from settings; defaults to the fixture source
            // while live InnerTube is unverified (see source.rs +
            // KNOWN-ISSUES.md).
            let source_mode = app_store.get_setting("source_mode").ok().flatten();
            let ytdlp_path = app_store.get_setting("ytdlp_path").ok().flatten();
            let source = AnySource::from_settings(source_mode, ytdlp_path);
            let player = player_ipc::PlayerSlot::new();

            // Accounts: metadata in accounts.db, the cookie itself in
            // the OS keyring (or an encrypted file when no keyring is
            // available — the store probe warns about that itself).
            let accounts_path = app_data_dir.join("accounts.db");
            let account_store = Arc::new(Mutex::new(AccountStore::open(&accounts_path).map_err(
                |e| {
                    format!(
                        "cannot open accounts store {}: {e}",
                        accounts_path.display()
                    )
                },
            )?));
            let (secrets, secret_backend) = open_secret_store(&app_data_dir.join("secrets"));
            let session = SharedSession::new(Session::default());
            let manager = Arc::new(SessionManager::new(
                Arc::clone(&account_store),
                secrets,
                session.clone(),
                Arc::new(TauriLoginFlow::new(app.handle().clone())),
            ));
            // Restore the saved session (blocking: the reads are short
            // SQLite/keyring calls, same as the heal-callback write
            // below). A failure just starts signed out.
            if let Err(e) = tauri::async_runtime::block_on(manager.load_active()) {
                tracing::warn!("could not restore the saved session ({e}); starting signed out");
            }
            // The background task that writes rotated cookies back to
            // the secret store. `spawn_cookie_sync` needs a runtime
            // context, so it runs inside this spawned task.
            let manager_for_sync = Arc::clone(&manager);
            let cookie_sync = tauri::async_runtime::spawn(async move {
                let _ = manager_for_sync.spawn_cookie_sync().await;
            });
            // The live feeds client over the new transport, with the
            // session healer attached. A failure to build it (bad
            // embedded client data) leaves feeds unavailable instead
            // of failing startup. The heal-callback write is a fast
            // lock acquisition, so blocking on it here is fine.
            let feeds = match Clients::embedded() {
                Ok(clients) => match Innertube::new(clients, session) {
                    Ok(client) => {
                        let client = tauri::async_runtime::block_on(
                            client.with_heal_callback(manager.heal_callback()),
                        );
                        Some(Arc::new(Feeds::new(Arc::new(client))))
                    }
                    Err(e) => {
                        tracing::warn!(
                            "cannot build InnerTube client ({e}); live feeds unavailable"
                        );
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("cannot load embedded clients ({e}); live feeds unavailable");
                    None
                }
            };

            app.manage(AppState {
                store: Mutex::new(app_store),
                source,
                player,
                accounts: manager,
                secret_backend,
                cookie_sync,
                feeds,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            search_videos,
            get_video,
            get_channel,
            get_playlist,
            get_playable,
            get_capabilities,
            report_video_rect,
            get_setting,
            set_setting,
            sign_in,
            sign_out,
            account_status,
            import_cookies,
            secret_backend,
            get_home_feed,
            list_subscriptions,
            subscribe,
            unsubscribe,
            list_watch_later,
            add_watch_later,
            remove_watch_later,
            list_history,
            record_history_entry,
            save_resume_position,
            get_resume_position,
            player_ipc::play_video,
            player_ipc::player_play,
            player_ipc::player_pause,
            player_ipc::player_toggle,
            player_ipc::player_seek,
            player_ipc::player_set_speed,
            player_ipc::player_set_audio_only,
            player_ipc::player_set_volume,
            player_ipc::player_set_muted,
            player_ipc::player_load_captions,
            player_ipc::player_clear_captions,
            player_ipc::player_status,
            player_ipc::player_next
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_path_is_tubelite_db_under_app_data_dir() {
        let dir = std::path::Path::new("/tmp/fake-app-data");
        assert_eq!(store_path(dir), dir.join("tubelite.db"));
    }

    #[test]
    fn store_persists_across_reopens() {
        // Regression test for the Phase 1 in-memory store: settings
        // written in one open must still be there in the next.
        let dir = std::env::temp_dir().join(format!("tubelite-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let store = open_app_store(&dir).unwrap();
            store.set_setting("quality", "720").unwrap();
            store
                .add_watch_later("vid1", "First", "Ch A", 1000)
                .unwrap();
        }
        let store = open_app_store(&dir).unwrap();
        assert_eq!(
            store.get_setting("quality").unwrap().as_deref(),
            Some("720")
        );
        assert_eq!(store.list_watch_later().unwrap().len(), 1);
        assert_eq!(store.schema_version().unwrap(), store::SCHEMA_VERSION);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
