//! PlayerService — the one owner of the live player.
//!
//! It binds the three pieces Phase 2 built separately: `MpvPlayer`
//! (the libmpv backend), `PlaybackState` (the pure state machine) and
//! `Queue`. src-tauri wraps this with IPC; the UI never talks to mpv
//! directly.
//!
//! Rules:
//! - The state machine is the single source of truth for speed,
//!   volume, pause and position *targets*; mpv owns the running clock.
//!   `sync_position()` pulls `time-pos` and `duration` from mpv into
//!   the state — call it on the position tick (4 Hz per the plan)
//!   before reading `position_secs`.
//! - Every command clamps through the state first, then applies to mpv,
//!   so a failed mpv call cannot leave the UI believing a different
//!   speed/position than the state says.
//! - `load_pair` resets position to 0 and applies the current
//!   audio-only flag and speed to the new item.

use crate::config::MpvConfig;
use crate::mpv_backend::MpvPlayer;
use crate::playback::PlaybackState;
use crate::queue::{Queue, QueueItem};

pub struct PlayerService {
    player: MpvPlayer,
    pub state: PlaybackState,
    pub queue: Queue,
}

impl PlayerService {
    pub fn new(config: &MpvConfig) -> Result<Self, libmpv2::Error> {
        Ok(Self {
            player: MpvPlayer::new(config)?,
            state: PlaybackState::default(),
            queue: Queue::default(),
        })
    }

    /// Raw mpv option override — used by the headless test (`vo`/`ao`
    /// to `null`) and as an escape hatch, not by normal playback code.
    pub fn set_option(&self, key: &str, value: &str) -> Result<(), libmpv2::Error> {
        self.player.set_option(key, value)
    }

    /// Load a DASH pair (or a single file when `audio_url` is None).
    /// Position resets to 0; the current audio-only flag and speed are
    /// applied to the new item.
    pub fn load_pair(
        &mut self,
        video_url: &str,
        audio_url: Option<&str>,
    ) -> Result<(), libmpv2::Error> {
        self.player.load_pair(video_url, audio_url)?;
        self.state.position_secs = 0.0;
        self.state.duration_secs = self.player.duration();
        if self.state.audio_only {
            self.player.set_audio_only(true)?;
        }
        if self.state.speed != 1.0 {
            self.player.set_speed(self.state.speed)?;
        }
        self.player
            .set_option("volume", &format!("{}", self.state.volume))?;
        self.player
            .set_option("mute", if self.state.muted { "yes" } else { "no" })?;
        Ok(())
    }

    /// Volume clamped to 0–100 by the state machine.
    pub fn set_volume(&mut self, volume: f64) -> Result<(), libmpv2::Error> {
        self.state.set_volume(volume);
        self.player
            .set_option("volume", &format!("{}", self.state.volume))
    }

    pub fn set_muted(&mut self, muted: bool) -> Result<(), libmpv2::Error> {
        self.state.muted = muted;
        self.player
            .set_option("mute", if muted { "yes" } else { "no" })
    }

    pub fn play(&mut self) -> Result<(), libmpv2::Error> {
        self.state.paused = false;
        self.player.set_pause(false)
    }

    pub fn pause(&mut self) -> Result<(), libmpv2::Error> {
        self.state.paused = true;
        self.player.set_pause(true)
    }

    pub fn toggle_pause(&mut self) -> Result<bool, libmpv2::Error> {
        if self.state.paused {
            self.play()?;
        } else {
            self.pause()?;
        }
        Ok(self.state.paused)
    }

    /// Absolute seek, clamped to the known duration by the state machine.
    pub fn seek_to(&mut self, secs: f64) -> Result<(), libmpv2::Error> {
        self.state.seek_to(secs);
        self.player.seek(self.state.position_secs)
    }

    pub fn seek_by(&mut self, delta_secs: f64) -> Result<(), libmpv2::Error> {
        self.seek_to(self.state.position_secs + delta_secs)
    }

    /// Speed clamped to 0.25–3x by the state machine.
    pub fn set_speed(&mut self, speed: f64) -> Result<(), libmpv2::Error> {
        self.state.set_speed(speed);
        self.player.set_speed(self.state.speed)
    }

    /// Load an external subtitle file; records the language in the state.
    pub fn load_subtitles_file(
        &mut self,
        path: &str,
        lang: Option<String>,
    ) -> Result<(), libmpv2::Error> {
        self.player.load_subtitles(path)?;
        self.state.subtitle_lang = lang;
        Ok(())
    }

    pub fn disable_subtitles(&mut self) -> Result<(), libmpv2::Error> {
        self.player.disable_subtitles()?;
        self.state.subtitle_lang = None;
        Ok(())
    }

    /// Audio-only toggle; returns the position playback resumes at.
    pub fn set_audio_only(&mut self, audio_only: bool) -> Result<f64, libmpv2::Error> {
        let resume_at = self.state.set_audio_only(audio_only);
        self.player.set_audio_only(audio_only)?;
        Ok(resume_at)
    }

    /// Pull mpv's clock into the state. Call on the position tick.
    pub fn sync_position(&mut self) {
        if let Some(pos) = self.player.time_pos() {
            self.state.position_secs = pos;
        }
        if let Some(dur) = self.player.duration() {
            self.state.duration_secs = Some(dur);
        }
    }

    pub fn position(&self) -> f64 {
        self.state.position_secs
    }

    pub fn avsync(&self) -> Option<f64> {
        self.player.avsync()
    }

    /// Advance the queue; the caller re-resolves the returned item's
    /// URLs and calls `load_pair`. Returns None at the end of the queue
    /// (RepeatMode decides whether `advance` itself wraps).
    pub fn advance_queue(&mut self) -> Option<&QueueItem> {
        self.queue.advance()
    }

    pub fn previous_queue(&mut self) -> Option<&QueueItem> {
        self.queue.previous()
    }
}
