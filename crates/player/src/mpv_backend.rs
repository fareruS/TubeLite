//! libmpv backend — compiled ONLY with `--features mpv`.
//!
//! Verified against libmpv2 **7.0.1** source (crate source in the local
//! cargo registry, checked 2026-11-08; GitHub releases v7.0.0/v7.0.1):
//! - `Mpv::new()`, `set_property`, `get_property`, `command(name, &[&str])`
//!   keep the signatures used here.
//! - `wait_event(timeout)` returns `Option<Result<Event>>`, NOT
//!   `Result<Option<Event>>` — the previous version of this file had that
//!   wrong; it would not have compiled against 7.x (or 6.x).
//! - `disable_deprecated_events()` exists; 7.0.0 fixed it being ignored,
//!   so call it at construction.
//! - 7.0.0 breaking change: a property whose format is `MPV_FORMAT_NONE`
//!   now returns `None` instead of panicking — callers reading a property
//!   must handle `None`, never unwrap a missing value.
//! - Render API: `Mpv::create_render_context(params)` replaces the old
//!   `RenderContext::new()` (removed in 6.0.0); the returned context borrows
//!   the `Mpv` and `OpenGLInitParams` requires a `'static` GL context.
//!   The render-context wiring itself is Phase 2 work, not claimed here.
//!
//! One long-lived mpv instance per window mode, reused across items.
//! Options come from `MpvConfig::as_options()`; each option's reason is
//! documented there, not repeated here.

use libmpv2::{events::Event, Mpv};

use crate::config::MpvConfig;

pub struct MpvPlayer {
    mpv: Mpv,
}

impl MpvPlayer {
    pub fn new(config: &MpvConfig) -> Result<Self, libmpv2::Error> {
        let mpv = Mpv::new()?;
        // 7.0.0 fixed this being a no-op; deprecated events must stay off
        // so the event pump only sees the current event set.
        mpv.disable_deprecated_events()?;
        for (key, value) in config.as_options() {
            mpv.set_property(key, value)?;
        }
        Ok(Self { mpv })
    }

    /// Load a DASH pair: video as the main file, audio as an external
    /// audio track on the same item, so one clock drives both.
    /// Verified-against-manual step for Phase 2: `audio-add` behaviour
    /// must be checked against the mpv manual for the pinned libmpv
    /// version before claiming A/V sync.
    pub fn load_pair(
        &self,
        video_url: &str,
        audio_url: Option<&str>,
    ) -> Result<(), libmpv2::Error> {
        self.mpv.command("loadfile", &[video_url, "replace"])?;
        if let Some(audio) = audio_url {
            // audio-add attaches the audio stream to the current file.
            self.mpv.command("audio-add", &[audio, "auto"])?;
        }
        Ok(())
    }

    /// Load an external subtitle file (e.g. a WebVTT written from a
    /// caption track) and select it.
    pub fn load_subtitles(&self, path: &str) -> Result<(), libmpv2::Error> {
        self.mpv.command("sub-add", &[path, "select"])
    }

    /// Turn subtitles off (`sid=no`).
    pub fn disable_subtitles(&self) -> Result<(), libmpv2::Error> {
        self.mpv.set_property("sid", "no")
    }

    pub fn set_audio_only(&self, audio_only: bool) -> Result<(), libmpv2::Error> {
        self.mpv
            .set_property("vid", if audio_only { "no" } else { "auto" })
    }

    pub fn set_pause(&self, pause: bool) -> Result<(), libmpv2::Error> {
        self.mpv.set_property("pause", pause)
    }

    /// Raw option override — used by headless tests (`vo=null`, `ao=null`)
    /// and by Settings-driven tuning the config struct does not cover.
    pub fn set_option(&self, key: &str, value: &str) -> Result<(), libmpv2::Error> {
        self.mpv.set_property(key, value)
    }

    /// Current duration in seconds, if mpv reports one.
    pub fn duration(&self) -> Option<f64> {
        self.mpv.get_property::<f64>("duration").ok()
    }

    /// Current audio/video sync offset in seconds, if mpv reports one.
    /// `avsync` is the difference between audio and video clocks; a
    /// growing magnitude after seeks means the DASH pair is drifting.
    pub fn avsync(&self) -> Option<f64> {
        self.mpv.get_property::<f64>("avsync").ok()
    }

    pub fn set_speed(&self, speed: f64) -> Result<(), libmpv2::Error> {
        // mpv pitch correction is on by default for `speed` (scaletempo).
        self.mpv.set_property("speed", speed)
    }

    pub fn seek(&self, secs: f64) -> Result<(), libmpv2::Error> {
        self.mpv.set_property("time-pos", secs)
    }

    /// Current playback position, if mpv reports one. In libmpv2 7.x a
    /// property with format MPV_FORMAT_NONE yields None — never unwrap.
    pub fn time_pos(&self) -> Option<f64> {
        self.mpv.get_property::<f64>("time-pos").ok()
    }

    /// Drain one event, if any. Returns `None` on timeout (no event).
    /// The app crate runs this on its own thread and forwards state to
    /// the UI via Tauri events, throttled to ~4 Hz for position ticks
    /// (discontinuities forwarded immediately).
    pub fn wait_event(&self, timeout_secs: f64) -> Option<Result<Event<'_>, libmpv2::Error>> {
        self.mpv.wait_event(timeout_secs)
    }

    /// Enable delivery of `MPV_EVENT_LOG_MESSAGE` on this handle, so
    /// the stage-1 player can see 403s in the mpv log. libmpv2 has no
    /// `mpv_request_log_messages` wrapper; `enable_event` is the
    /// mechanism `enable_all_events` uses for the same id.
    pub fn enable_log_messages(&self) -> Result<(), libmpv2::Error> {
        self.mpv
            .enable_event(libmpv2::events::mpv_event_id::LogMessage)
    }

    /// Create the render context for video. The caller owns the GL context
    /// (must be `'static`); the returned context borrows this player.
    /// Not used until the Phase 2 render wiring lands — kept here so the
    /// 7.x API shape is pinned in code, not just in a doc.
    pub fn create_render_context<C: 'static>(
        &self,
        params: impl IntoIterator<Item = libmpv2::render::RenderParam<C>>,
    ) -> Result<libmpv2::render::RenderContext<'_>, libmpv2::Error> {
        self.mpv.create_render_context(params)
    }
}
