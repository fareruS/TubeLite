//! Player IPC — the UI's only way to drive playback.
//!
//! `PlayerSlot` holds the `PlayerService` behind the `mpv` feature.
//! Without the feature (default build, CI) the slot is `Disabled` and
//! every command returns a clear error instead of failing the build or
//! the app — playback is a build-time capability, not a runtime crash.
//! A failed libmpv initialisation also degrades to `Disabled` with a
//! warning log; startup never fails because the player could not start.
//!
//! Commands: `play_video` (resolve via the source chain, then load),
//! `player_play` / `player_pause` / `player_toggle`, `player_seek`,
//! `player_set_speed`, `player_set_audio_only`, `player_status`
//! (syncs mpv's clock into the state, 4 Hz tick per the plan),
//! `player_next` (advances the queue and returns the next video id;
//! the UI then calls `play_video` for it).

#[cfg(feature = "mpv")]
use std::sync::Mutex;

use serde::Serialize;
use tauri::{State, WebviewWindow};
use yt_model::{QualityPref, VideoId};

use crate::{check_window, AppState};
use yt_source::StreamSource;

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

pub enum PlayerSlot {
    Disabled,
    /// Boxed so the variants stay close in size
    /// (clippy::large_enum_variant).
    #[cfg(feature = "mpv")]
    Ready(Box<Mutex<player::PlayerService>>),
}

impl Default for PlayerSlot {
    fn default() -> Self {
        Self::new()
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

impl PlayerSlot {
    pub fn new() -> Self {
        #[cfg(feature = "mpv")]
        {
            let cache_dir = cache_dir();
            match player::PlayerService::new(&player::MpvConfig::video(cache_dir)) {
                Ok(service) => return Self::Ready(Box::new(Mutex::new(service))),
                Err(e) => {
                    tracing::warn!("libmpv failed to initialise ({e}); playback disabled");
                }
            }
        }
        Self::Disabled
    }

    const DISABLED_MSG: &'static str = "playback unavailable: this build was compiled without the `mpv` feature, or libmpv failed to initialise";

    /// Run `f` against the live service, or return the disabled error.
    /// mpv-only: without the feature there is no `PlayerService` type,
    /// so callers use the `PlayerSlot` methods below, whose bodies are
    /// cfg-gated instead of routing through this helper.
    #[cfg(feature = "mpv")]
    pub fn with_service<R>(
        &self,
        f: impl FnOnce(&mut player::PlayerService) -> Result<R, String>,
    ) -> Result<R, String> {
        match self {
            Self::Disabled => Err(Self::DISABLED_MSG.into()),
            Self::Ready(mutex) => {
                let mut service = mutex
                    .lock()
                    .map_err(|e| format!("player lock poisoned: {e}"))?;
                f(&mut service)
            }
        }
    }

    /// Drive one command against the service and return the fresh status.
    /// Each method is a single cfg-gated body so the non-mpv build never
    /// names the `PlayerService` type.
    pub fn play(&self) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.play().map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn pause(&self) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.pause().map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn toggle(&self) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.toggle_pause().map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn seek_to(&self, secs: f64) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.seek_to(secs).map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            let _ = secs;
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn set_speed(&self, speed: f64) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.set_speed(speed).map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            let _ = speed;
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn set_audio_only(&self, audio_only: bool) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.set_audio_only(audio_only).map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            let _ = audio_only;
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn load_subtitles_file(
        &self,
        path: &str,
        lang: Option<String>,
    ) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.load_subtitles_file(path, lang)
                    .map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            let _ = (path, lang);
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn clear_subtitles(&self) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.disable_subtitles().map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn set_volume(&self, volume: f64) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.set_volume(volume).map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            let _ = volume;
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn set_muted(&self, muted: bool) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.set_muted(muted).map_err(|e| e.to_string())?;
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            let _ = muted;
            Err(Self::DISABLED_MSG.into())
        }
    }

    pub fn next_id(&self) -> Result<Option<String>, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| Ok(svc.advance_queue().map(|item| item.video_id.0.clone())))
        }
        #[cfg(not(feature = "mpv"))]
        {
            Err(Self::DISABLED_MSG.into())
        }
    }

    /// Load a resolved DASH pair into the service and start playing.
    /// `audio_only` / `quality_height` are applied to the state before
    /// the load so the new item starts with the right settings.
    #[cfg(feature = "mpv")]
    pub fn load_and_play(
        &self,
        video_url: String,
        audio_url: Option<String>,
        id: VideoId,
        audio_only: bool,
        quality_height: Option<u32>,
    ) -> Result<PlayerStatus, String> {
        self.with_service(|svc| {
            svc.queue = player::Queue::new(vec![player::QueueItem {
                video_id: id,
                title: String::new(),
                resume_secs: None,
            }]);
            svc.state.set_audio_only(audio_only);
            svc.state.quality_switch(quality_height);
            svc.load_pair(&video_url, audio_url.as_deref())
                .map_err(|e| e.to_string())?;
            svc.play().map_err(|e| e.to_string())?;
            Ok(status_of(svc))
        })
    }

    pub fn status(&self) -> Result<PlayerStatus, String> {
        #[cfg(feature = "mpv")]
        {
            self.with_service(|svc| {
                svc.sync_position();
                Ok(status_of(svc))
            })
        }
        #[cfg(not(feature = "mpv"))]
        {
            Err(Self::DISABLED_MSG.into())
        }
    }
}

#[cfg(feature = "mpv")]
fn status_of(svc: &mut player::PlayerService) -> PlayerStatus {
    PlayerStatus {
        position_secs: svc.state.position_secs,
        duration_secs: svc.state.duration_secs,
        paused: svc.state.paused,
        speed: svc.state.speed,
        volume: svc.state.volume,
        muted: svc.state.muted,
        audio_only: svc.state.audio_only,
        quality_height: svc.state.quality_height,
    }
}

fn pref_from_height(height: Option<u32>) -> QualityPref {
    match height {
        None => QualityPref::AudioOnly,
        Some(0) => QualityPref::Auto,
        Some(h) => QualityPref::Height(h),
    }
}

/// Pick the URLs mpv should load for a resolved `PlayableMedia`.
/// Returns (main_url, extra_audio_url). In audio-only mode there is no
/// video format — the audio stream itself is the main file, which mpv
/// plays standalone; the old code errored here ("no video stream"),
/// which made audio-only playback via `play_video` impossible.
fn urls_for_pref(
    media: &yt_model::PlayableMedia,
    pref: QualityPref,
) -> Result<(String, Option<String>), String> {
    match pref {
        QualityPref::AudioOnly => {
            let audio = media
                .audio
                .as_ref()
                .map(|f| f.url.clone())
                .ok_or_else(|| "no audio stream for this video".to_string())?;
            Ok((audio, None))
        }
        _ => {
            let video = media
                .video
                .as_ref()
                .map(|f| f.url.clone())
                .ok_or_else(|| "no video stream for this quality".to_string())?;
            let audio = media.audio.as_ref().map(|f| f.url.clone());
            Ok((video, audio))
        }
    }
}

#[tauri::command]
pub async fn play_video(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
    height: Option<u32>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    let pref = pref_from_height(height);
    let media = state
        .source
        .playable(&VideoId(id.clone()), pref)
        .await
        .map_err(|e| e.to_string())?;
    let (video_url, audio_url) = urls_for_pref(&media, pref)?;
    let (audio_only, quality_height) = match pref {
        QualityPref::AudioOnly => (true, None),
        QualityPref::Auto => (false, Some(720)),
        QualityPref::Height(h) => (false, Some(h)),
    };
    #[cfg(feature = "mpv")]
    {
        state.player.load_and_play(
            video_url,
            audio_url,
            VideoId(id),
            audio_only,
            quality_height,
        )
    }
    #[cfg(not(feature = "mpv"))]
    {
        let _ = (video_url, audio_url, audio_only, quality_height);
        Err("playback unavailable: this build was compiled without the `mpv` feature".into())
    }
}

#[tauri::command]
pub fn player_play(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.play()
}

#[tauri::command]
pub fn player_pause(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.pause()
}

#[tauri::command]
pub fn player_toggle(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.toggle()
}

#[tauri::command]
pub fn player_seek(
    window: WebviewWindow,
    state: State<'_, AppState>,
    secs: f64,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.seek_to(secs)
}

#[tauri::command]
pub fn player_set_speed(
    window: WebviewWindow,
    state: State<'_, AppState>,
    speed: f64,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.set_speed(speed)
}

#[tauri::command]
pub fn player_set_audio_only(
    window: WebviewWindow,
    state: State<'_, AppState>,
    audio_only: bool,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.set_audio_only(audio_only)
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
    state
        .player
        .load_subtitles_file(&path_str, Some(lang.clone()))?;
    Ok(track.language_name.clone())
}

#[tauri::command]
pub fn player_set_volume(
    window: WebviewWindow,
    state: State<'_, AppState>,
    volume: f64,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.set_volume(volume)
}

#[tauri::command]
pub fn player_set_muted(
    window: WebviewWindow,
    state: State<'_, AppState>,
    muted: bool,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.set_muted(muted)
}

#[tauri::command]
pub fn player_clear_captions(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.clear_subtitles()
}

#[tauri::command]
pub fn player_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<PlayerStatus, String> {
    check_window(&window)?;
    state.player.status()
}

#[tauri::command]
pub fn player_next(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    check_window(&window)?;
    state.player.next_id()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_media(with_video: bool) -> yt_model::PlayableMedia {
        yt_model::PlayableMedia {
            video_id: VideoId("dQw4w9WgXcQ".into()),
            video: with_video.then(|| yt_model::Format {
                itag: 247,
                mime_type: "video/webm".into(),
                height: Some(720),
                bitrate: Some(2_500_000),
                url: "https://example.invalid/video".into(),
                expires_at_unix: Some(1_900_000_000),
            }),
            audio: Some(yt_model::Format {
                itag: 251,
                mime_type: "audio/webm".into(),
                height: None,
                bitrate: Some(160_000),
                url: "https://example.invalid/audio".into(),
                expires_at_unix: Some(1_900_000_000),
            }),
            candidates: vec![],
            expires_at_unix: Some(1_900_000_000),
        }
    }

    #[test]
    fn urls_for_pref_audio_only_uses_audio_as_main_file() {
        let media = fake_media(false); // no video format, as sources return
        let (main, extra) = urls_for_pref(&media, QualityPref::AudioOnly).unwrap();
        assert_eq!(main, "https://example.invalid/audio");
        assert_eq!(extra, None);
    }

    #[test]
    fn urls_for_pref_video_returns_pair() {
        let media = fake_media(true);
        let (main, extra) = urls_for_pref(&media, QualityPref::Height(720)).unwrap();
        assert_eq!(main, "https://example.invalid/video");
        assert_eq!(extra.as_deref(), Some("https://example.invalid/audio"));
    }

    #[test]
    fn urls_for_pref_errors_when_stream_missing() {
        let mut media = fake_media(false);
        media.audio = None;
        assert!(urls_for_pref(&media, QualityPref::AudioOnly).is_err());
        assert!(urls_for_pref(&media, QualityPref::Height(720)).is_err());
    }

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

    #[cfg(not(feature = "mpv"))]
    #[test]
    fn slot_disabled_without_feature() {
        let slot = PlayerSlot::new();
        assert!(matches!(slot, PlayerSlot::Disabled));
        let err = slot.status().unwrap_err();
        assert!(err.contains("mpv"), "disabled error should name mpv: {err}");
    }

    #[cfg(feature = "mpv")]
    #[test]
    fn slot_drives_fixture() {
        let slot = PlayerSlot::new();
        slot.with_service(|svc| {
            svc.set_option("vo", "null").map_err(|e| e.to_string())?;
            svc.set_option("ao", "null").map_err(|e| e.to_string())?;
            Ok(())
        })
        .unwrap();
        let fixture = format!(
            "{}/../crates/player/tests/fixtures/test-60s.mp4",
            env!("CARGO_MANIFEST_DIR")
        );
        let url = format!("file://{fixture}");
        let status = slot
            .load_and_play(url, None, VideoId("test-fixture".into()), false, Some(720))
            .unwrap();
        assert!(!status.paused);
        std::thread::sleep(std::time::Duration::from_secs(1));
        let status = slot.status().unwrap();
        assert!(
            status.position_secs > 0.5,
            "position did not advance: {}",
            status.position_secs
        );
        assert_eq!(status.duration_secs.map(|d| d.round()), Some(60.0));
    }
}
