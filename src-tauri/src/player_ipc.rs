//! Player IPC — the UI's only way to drive playback.
//!
//! `PlayerSlot` holds the new `player::Player` (yt-dlp resolution via
//! [`SessionResolver`], mpv behind `MpvControlImpl`) behind the `mpv`
//! feature. Without the feature (default build, CI) the slot is
//! `Disabled` and every command returns a clear error instead of
//! failing the build or the app — playback is a build-time capability,
//! not a runtime crash. A failed libmpv initialisation also degrades
//! to `Disabled` with a warning log; startup never fails because the
//! player could not start.
//!
//! v0.1.0 video: the stage-1 mpv options
//! (`MpvConfig::stage1_options`, applied by `Player::new`) set NO `vo`
//! key, so mpv opens its own window for video instead of rendering
//! into the webview — the embedded video surface is unbuilt, and the
//! `vo=libmpv` the old path used has no render context yet. See
//! `MpvControlImpl::new`, which overrides the `vo=libmpv` that
//! `MpvPlayer::new` applies back to `gpu` for this reason. Embedded
//! video (render context under the webview) is the follow-up; when it
//! lands, delete that override and this paragraph.
//!
//! Commands (names and the `PlayerStatus` shape are the UI contract —
//! `ui/src/lib/player.ts` invokes them and must not change):
//! `play_video` (resolve via yt-dlp, then load), `player_play` /
//! `player_pause` / `player_toggle`, `player_seek`,
//! `player_set_speed`, `player_set_volume`, `player_set_muted`,
//! `player_set_audio_only`, `player_status` (reads the new Player's
//! state, which its event loop keeps fresh), `player_next` (no queue
//! is carried over by the new Player, so it returns `Ok(None)`).
//! Speed / volume / mute / audio-only are UI-side state applied to
//! mpv with `set_option`; mpv properties persist across `loadfile`,
//! so a value set before any playback still applies to the next item.

#[cfg(feature = "mpv")]
use std::path::PathBuf;
#[cfg(feature = "mpv")]
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use innertube::SharedSession;
#[cfg(feature = "mpv")]
use player::PlayerState;
use serde::Serialize;
use tauri::{State, WebviewWindow};
#[cfg(feature = "mpv")]
use yt_model::VideoId;
#[cfg(feature = "mpv")]
use yt_source::StreamSource;

use crate::{check_window, AppState};

#[cfg(feature = "mpv")]
use player::{Resolver, YtDlpResolver};
#[cfg(feature = "mpv")]
use std::future::Future;
#[cfg(feature = "mpv")]
use std::pin::Pin;

#[derive(Debug, Clone, Serialize)]
pub struct PlayerStatus {
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
    pub paused: bool,
    pub speed: f64,
    pub volume: f64,
    pub muted: bool,
    pub audio_only: bool,
    pub quality_height: Option<u32>,
}

/// Convert a `Cookie` header (`NAME=value; NAME=value`) into a
/// Netscape cookie-file export for `.youtube.com`, the shape
/// `YtDlpResolver::with_cookies` takes. Pairs with an empty value
/// (deletions) and malformed pairs are skipped. Returns `None` when
/// no usable pair survives. Cookie *values* are copied, never logged.
/// (Only called from the `mpv` path; kept compiled in all builds so
/// its tests run without libmpv.)
#[cfg_attr(not(feature = "mpv"), allow(dead_code))]
fn cookie_header_to_netscape(header: &str) -> Option<String> {
    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() + 365 * 24 * 3600)
        .unwrap_or(0);
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    let mut any = false;
    for pair in header.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty()
            || value.is_empty()
            || name.contains(['\t', '\n', '\r'])
            || value.contains(['\t', '\n', '\r'])
        {
            continue;
        }
        // domain, flag (subdomains included), path, secure (all
        // requests are HTTPS), expiry, name, value.
        out.push_str(&format!(
            ".youtube.com\tTRUE\t/\tTRUE\t{expiry}\t{name}\t{value}\n"
        ));
        any = true;
    }
    if any {
        Some(out)
    } else {
        None
    }
}

/// yt-dlp binary, in order: the `ytdlp_path` setting, a sidecar next
/// to the current exe (the shipped zip puts `yt-dlp.exe` beside
/// `TubeLite.exe`), otherwise `yt-dlp` on `PATH`. A missing binary
/// surfaces the resolver's error naming the install/update command.
#[cfg(feature = "mpv")]
fn resolve_ytdlp_path(setting: Option<String>) -> PathBuf {
    if let Some(path) = setting.filter(|p| !p.trim().is_empty()) {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["yt-dlp.exe", "yt-dlp"] {
                let sidecar = dir.join(name);
                if sidecar.exists() {
                    return sidecar;
                }
            }
        }
    }
    PathBuf::from("yt-dlp")
}

/// A [`Resolver`] that builds a fresh [`YtDlpResolver`] on every
/// resolve, feeding it the CURRENT cookie header from the shared
/// accounts session — so a sign-in (or a cookie rotation) after
/// startup is picked up by the next resolve. Cookies come only from
/// the live session, never from a file on disk and never hardcoded;
/// the resolver itself writes them to a 0600 temp file that lives
/// only for the resolve call.
#[cfg(feature = "mpv")]
struct SessionResolver {
    session: SharedSession,
    yt_dlp: PathBuf,
}

#[cfg(feature = "mpv")]
impl SessionResolver {
    fn new(session: SharedSession, yt_dlp: PathBuf) -> Self {
        Self { session, yt_dlp }
    }
}

#[cfg(feature = "mpv")]
impl Resolver for SessionResolver {
    fn resolve(
        &self,
        video_id: &str,
        max_height: u32,
    ) -> Pin<Box<dyn Future<Output = Result<player::Resolved, player::ResolveError>> + Send + '_>>
    {
        let video_id = video_id.to_string();
        Box::pin(async move {
            let mut resolver = YtDlpResolver::new(self.yt_dlp.clone());
            if let Some(header) = self.session.cookie().await {
                if let Some(netscape) = cookie_header_to_netscape(&header) {
                    resolver = resolver.with_cookies(netscape);
                }
            }
            resolver.resolve(&video_id, max_height).await
        })
    }
}

/// UI-side playback settings the new `player::Player` does not own
/// (speed, volume, mute, audio-only, quality). Applied to mpv with
/// `set_option` and kept here so `player_status` can report them.
#[cfg(feature = "mpv")]
struct SlotUi {
    speed: f64,
    volume: f64,
    muted: bool,
    audio_only: bool,
    quality_height: Option<u32>,
}

#[cfg(feature = "mpv")]
impl Default for SlotUi {
    fn default() -> Self {
        Self {
            speed: 1.0,
            volume: 100.0,
            muted: false,
            audio_only: false,
            quality_height: Some(720),
        }
    }
}

#[cfg(feature = "mpv")]
pub struct ReadyPlayer {
    player: player::Player,
    ui: Mutex<SlotUi>,
}

#[cfg(feature = "mpv")]
impl ReadyPlayer {
    async fn status(&self) -> Result<PlayerStatus, String> {
        let state = self.player.state().await;
        let position = self.player.position().await;
        let duration = self.player.duration().await;
        let ui = self
            .ui
            .lock()
            .map_err(|e| format!("player lock poisoned: {e}"))?;
        Ok(PlayerStatus {
            position_secs: position,
            duration_secs: duration,
            paused: state == PlayerState::Paused,
            speed: ui.speed,
            volume: ui.volume,
            muted: ui.muted,
            audio_only: ui.audio_only,
            quality_height: ui.quality_height,
        })
    }
}

pub enum PlayerSlot {
    Disabled,
    /// Boxed so the variants stay close in size
    /// (clippy::large_enum_variant).
    #[cfg(feature = "mpv")]
    Ready(Box<ReadyPlayer>),
}

impl PlayerSlot {
    pub fn new(session: SharedSession, ytdlp_path: Option<String>) -> Self {
        #[cfg(feature = "mpv")]
        {
            let resolver = Arc::new(SessionResolver::new(
                session,
                resolve_ytdlp_path(ytdlp_path),
            ));
            let cache = cache_dir();
            match player::MpvControlImpl::new(&player::MpvConfig::video(cache.clone())) {
                Ok(control) => {
                    match player::Player::new(
                        resolver,
                        Box::new(control),
                        PathBuf::from(cache),
                        720,
                    ) {
                        Ok(player) => {
                            // Spawn the event pump: it polls mpv's
                            // position, drives URL-failure recovery,
                            // and keeps the state `player_status`
                            // reads fresh. Runs until the app exits.
                            let pump = player.clone();
                            tauri::async_runtime::spawn(async move {
                                pump.run_event_loop().await;
                            });
                            return Self::Ready(Box::new(ReadyPlayer {
                                player,
                                ui: Mutex::new(SlotUi::default()),
                            }));
                        }
                        Err(e) => {
                            tracing::warn!("player failed to initialise ({e}); playback disabled");
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("libmpv failed to initialise ({e}); playback disabled");
                }
            }
        }
        #[cfg(not(feature = "mpv"))]
        {
            let _ = (session, ytdlp_path);
        }
        Self::Disabled
    }

    const DISABLED_MSG: &'static str = "playback unavailable: this build was compiled without the `mpv` feature, or libmpv failed to initialise";

    pub async fn play(&self) -> Result<PlayerStatus, String> {
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                ready.player.resume().await.map_err(|e| e.to_string())?;
                ready.status().await
            }
        }
    }

    pub async fn pause(&self) -> Result<PlayerStatus, String> {
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                ready.player.pause().await.map_err(|e| e.to_string())?;
                ready.status().await
            }
        }
    }

    pub async fn toggle(&self) -> Result<PlayerStatus, String> {
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                match ready.player.state().await {
                    PlayerState::Paused => {
                        ready.player.resume().await.map_err(|e| e.to_string())?;
                    }
                    PlayerState::Playing => {
                        ready.player.pause().await.map_err(|e| e.to_string())?;
                    }
                    // Idle / Loading / Ended / Error: nothing to toggle.
                    _ => {}
                }
                ready.status().await
            }
        }
    }

    pub async fn seek_to(&self, secs: f64) -> Result<PlayerStatus, String> {
        #[cfg(not(feature = "mpv"))]
        let _ = secs;
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                ready.player.seek(secs).await.map_err(|e| e.to_string())?;
                ready.status().await
            }
        }
    }

    pub async fn set_speed(&self, speed: f64) -> Result<PlayerStatus, String> {
        #[cfg(not(feature = "mpv"))]
        let _ = speed;
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                let speed = speed.clamp(0.25, 3.0);
                ready
                    .player
                    .set_option("speed", &speed.to_string())
                    .await
                    .map_err(|e| e.to_string())?;
                {
                    let mut ui = ready
                        .ui
                        .lock()
                        .map_err(|e| format!("player lock poisoned: {e}"))?;
                    ui.speed = speed;
                }
                ready.status().await
            }
        }
    }

    pub async fn set_audio_only(&self, audio_only: bool) -> Result<PlayerStatus, String> {
        #[cfg(not(feature = "mpv"))]
        let _ = audio_only;
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                ready
                    .player
                    .set_option("vid", if audio_only { "no" } else { "auto" })
                    .await
                    .map_err(|e| e.to_string())?;
                {
                    let mut ui = ready
                        .ui
                        .lock()
                        .map_err(|e| format!("player lock poisoned: {e}"))?;
                    ui.audio_only = audio_only;
                    if audio_only {
                        ui.quality_height = None;
                    } else if ui.quality_height.is_none() {
                        ui.quality_height = Some(720);
                    }
                }
                ready.status().await
            }
        }
    }

    pub async fn load_subtitles(&self, path: &str) -> Result<PlayerStatus, String> {
        #[cfg(not(feature = "mpv"))]
        let _ = path;
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                ready
                    .player
                    .load_subtitles(path)
                    .await
                    .map_err(|e| e.to_string())?;
                ready.status().await
            }
        }
    }

    pub async fn clear_subtitles(&self) -> Result<PlayerStatus, String> {
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                ready
                    .player
                    .disable_subtitles()
                    .await
                    .map_err(|e| e.to_string())?;
                ready.status().await
            }
        }
    }

    pub async fn set_volume(&self, volume: f64) -> Result<PlayerStatus, String> {
        #[cfg(not(feature = "mpv"))]
        let _ = volume;
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                let volume = volume.clamp(0.0, 100.0);
                ready
                    .player
                    .set_option("volume", &volume.to_string())
                    .await
                    .map_err(|e| e.to_string())?;
                {
                    let mut ui = ready
                        .ui
                        .lock()
                        .map_err(|e| format!("player lock poisoned: {e}"))?;
                    ui.volume = volume;
                }
                ready.status().await
            }
        }
    }

    pub async fn set_muted(&self, muted: bool) -> Result<PlayerStatus, String> {
        #[cfg(not(feature = "mpv"))]
        let _ = muted;
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                ready
                    .player
                    .set_option("mute", if muted { "yes" } else { "no" })
                    .await
                    .map_err(|e| e.to_string())?;
                {
                    let mut ui = ready
                        .ui
                        .lock()
                        .map_err(|e| format!("player lock poisoned: {e}"))?;
                    ui.muted = muted;
                }
                ready.status().await
            }
        }
    }

    pub async fn next_id(&self) -> Result<Option<String>, String> {
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(_) => Ok(None),
        }
    }

    /// Resolve via yt-dlp and play from the start. `audio_only`
    /// resolves at 144p (the video track is fetched but never
    /// decoded — `vid=no` — so the smallest height wastes the least
    /// bandwidth); otherwise `max_height` is the resolve cap and the
    /// reported quality. Resolve errors (including a missing yt-dlp,
    /// which names the install/update command) surface as strings.
    pub async fn play_video(
        &self,
        id: &str,
        audio_only: bool,
        max_height: u32,
    ) -> Result<PlayerStatus, String> {
        #[cfg(not(feature = "mpv"))]
        let _ = (id, audio_only, max_height);
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => {
                ready.player.set_max_height(max_height).await;
                ready
                    .player
                    .set_option("vid", if audio_only { "no" } else { "auto" })
                    .await
                    .map_err(|e| e.to_string())?;
                ready.player.play(id).await.map_err(|e| e.to_string())?;
                {
                    let mut ui = ready
                        .ui
                        .lock()
                        .map_err(|e| format!("player lock poisoned: {e}"))?;
                    ui.audio_only = audio_only;
                    ui.quality_height = if audio_only { None } else { Some(max_height) };
                }
                ready.status().await
            }
        }
    }

    pub async fn status(&self) -> Result<PlayerStatus, String> {
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            #[cfg(feature = "mpv")]
            Self::Ready(ready) => ready.status().await,
        }
    }
}

/// On-disk cache directory (mpv cache, downloaded caption files).
/// Overridable with TUBELITE_CACHE_DIR for tests.
pub fn cache_dir() -> String {
    std::env::var("TUBELITE_CACHE_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        format!("{home}/.cache/tubelite")
    })
}

#[tauri::command]
pub async fn play_video(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
    height: Option<u32>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    // Same mapping the old `play_video` used: no height means
    // audio-only, 0 means auto (720p), anything else is the cap.
    let (audio_only, max_height) = match height {
        None => (true, 144),
        Some(0) => (false, 720),
        Some(h) => (false, h),
    };
    state.player.play_video(&id, audio_only, max_height).await
}

#[tauri::command]
pub async fn player_play(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.play().await
}

#[tauri::command]
pub async fn player_pause(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.pause().await
}

#[tauri::command]
pub async fn player_toggle(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.toggle().await
}

#[tauri::command]
pub async fn player_seek(
    window: WebviewWindow,
    state: State<'_, AppState>,
    secs: f64,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.seek_to(secs).await
}

#[tauri::command]
pub async fn player_set_speed(
    window: WebviewWindow,
    state: State<'_, AppState>,
    speed: f64,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.set_speed(speed).await
}

#[tauri::command]
pub async fn player_set_audio_only(
    window: WebviewWindow,
    state: State<'_, AppState>,
    audio_only: bool,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.set_audio_only(audio_only).await
}

/// Fetch a caption track for the video, convert it to WebVTT in the
/// cache dir, and load it into mpv. The track URL is never logged.
#[tauri::command]
#[cfg(not(feature = "mpv"))]
pub async fn player_load_captions(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
    lang: String,
) -> Result<String, String> {
    check_window(&window)?;
    let _ = (&state, id, lang);
    Err("playback unavailable: this build was compiled without the `mpv` feature".into())
}

#[tauri::command]
#[cfg(feature = "mpv")]
pub async fn player_load_captions(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
    lang: String,
) -> Result<String, String> {
    check_window(&window)?;
    let details = state
        .source
        .video(&VideoId(id.clone()))
        .await
        .map_err(|e| e.to_string())?;
    let track = details
        .captions
        .iter()
        .find(|t| t.language_code == lang)
        .ok_or_else(|| format!("no caption track for language '{lang}'"))?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("TubeLite/0.1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let cues = yt_source::fetch_captions(&client, &track.base_url)
        .await
        .map_err(|e| e.to_string())?;
    if cues.is_empty() {
        return Err(format!("caption track '{lang}' has no cues"));
    }
    let dir = std::path::PathBuf::from(cache_dir()).join("captions");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Video ids are validated to 11 [A-Za-z0-9_-] chars by the
    // sources, and lang comes from the track list — but sanitise
    // anyway so the filename can never escape the cache dir.
    let safe_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let safe_lang: String = lang
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let path = dir.join(format!("{safe_id}-{safe_lang}.vtt"));
    std::fs::write(&path, yt_source::to_vtt(&cues)).map_err(|e| e.to_string())?;
    let path_str = path.to_string_lossy().into_owned();
    state.player.load_subtitles(&path_str).await?;
    Ok(track.language_name.clone())
}

#[tauri::command]
pub async fn player_set_volume(
    window: WebviewWindow,
    state: State<'_, AppState>,
    volume: f64,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.set_volume(volume).await
}

#[tauri::command]
pub async fn player_set_muted(
    window: WebviewWindow,
    state: State<'_, AppState>,
    muted: bool,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.set_muted(muted).await
}

#[tauri::command]
pub async fn player_clear_captions(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.clear_subtitles().await
}

#[tauri::command]
pub async fn player_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.status().await
}

#[tauri::command]
pub async fn player_next(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    check_window(&window)?;
    state.player.next_id().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes() {
        let s = PlayerStatus {
            position_secs: 12.5,
            duration_secs: Some(213.0),
            paused: false,
            speed: 1.5,
            volume: 80.0,
            muted: false,
            audio_only: false,
            quality_height: Some(720),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"position_secs\":12.5"));
        assert!(json.contains("\"quality_height\":720"));
    }

    #[test]
    fn cookie_header_to_netscape_converts_pairs() {
        let netscape =
            cookie_header_to_netscape("SAPISID=abc123; SID=xyz; __Secure-3PAPISID=qrs").unwrap();
        assert!(netscape.starts_with("# Netscape HTTP Cookie File\n"));
        for (name, value) in [
            ("SAPISID", "abc123"),
            ("SID", "xyz"),
            ("__Secure-3PAPISID", "qrs"),
        ] {
            assert!(
                netscape.contains(".youtube.com\tTRUE\t/\tTRUE\t"),
                "missing .youtube.com line for {name}"
            );
            assert!(
                netscape.contains(&format!("\t{name}\t{value}\n")),
                "missing {name}={value} line"
            );
        }
    }

    #[test]
    fn cookie_header_to_netscape_skips_empty_and_malformed() {
        assert!(cookie_header_to_netscape("").is_none());
        assert!(cookie_header_to_netscape("SID=; SAPISID=").is_none());
        assert!(cookie_header_to_netscape("not-a-pair").is_none());
        let netscape = cookie_header_to_netscape("SID=; SAPISID=abc").unwrap();
        assert!(netscape.contains("\tSAPISID\tabc\n"));
        assert!(!netscape.contains("SID"));
    }

    #[cfg(feature = "mpv")]
    #[test]
    fn ytdlp_path_prefers_setting_then_sidecar_then_path() {
        assert_eq!(
            resolve_ytdlp_path(Some("/custom/yt-dlp".into())),
            PathBuf::from("/custom/yt-dlp")
        );
        assert_eq!(
            resolve_ytdlp_path(Some("   ".into())).file_name().unwrap(),
            "yt-dlp"
        );
        // No sidecar sits next to the test binary: falls back to PATH.
        assert_eq!(resolve_ytdlp_path(None).file_name().unwrap(), "yt-dlp");
    }

    #[cfg(not(feature = "mpv"))]
    #[tokio::test]
    async fn slot_disabled_without_feature() {
        let slot = PlayerSlot::new(SharedSession::default(), None);
        assert!(matches!(slot, PlayerSlot::Disabled));
        let err = slot.status().await.unwrap_err();
        assert!(err.contains("mpv"), "disabled error should name mpv: {err}");
        assert!(slot.next_id().await.is_err());
    }

    #[cfg(feature = "mpv")]
    #[tokio::test]
    async fn slot_reports_defaults_and_clamps_speed() {
        let slot = PlayerSlot::new(SharedSession::default(), None);
        // No libmpv in this environment: construction degrades to
        // Disabled instead of failing. On a machine with libmpv it is
        // Ready and the assertions below run.
        let PlayerSlot::Ready(_) = &slot else {
            return;
        };
        let status = slot.status().await.unwrap();
        assert_eq!(status.position_secs, 0.0);
        assert!(!status.paused);
        assert_eq!(status.speed, 1.0);
        assert_eq!(status.volume, 100.0);
        assert_eq!(status.quality_height, Some(720));
        let status = slot.set_speed(99.0).await.unwrap();
        assert_eq!(status.speed, 3.0, "speed clamps to 0.25–3.0");
        let status = slot.set_speed(0.1).await.unwrap();
        assert_eq!(status.speed, 0.25);
    }
}
