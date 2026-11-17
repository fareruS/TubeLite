//! The real [`MpvControl`] adapter over [`MpvPlayer`] — compiled
//! ONLY with `--features mpv`.
//!
//! Log-message delivery: libmpv2 7.0.1 parses `MPV_EVENT_LOG_MESSAGE`
//! in `wait_event`, but exposes no `mpv_request_log_messages`
//! wrapper. This adapter enables the event with
//! `Mpv::enable_event(EventId::LogMessage)` (the same mechanism
//! `enable_all_events` uses, which includes the log-message id), so
//! 403s in the log reach the recovery path. If a future libmpv
//! version stops delivering log messages without an explicit
//! `mpv_request_log_messages` call, the 403-log path degrades
//! gracefully: the end-file-error path still triggers recovery,
//! because a 403 makes the demuxer fail and end the file with
//! reason `ERROR`.

use std::path::Path;
use std::time::Duration;

use libmpv2::events::Event;
use libmpv2::mpv_end_file_reason;

use crate::config::MpvConfig;
use crate::mpv_backend::MpvPlayer;
use crate::player::{MpvControl, MpvEvent, PlayerError};

pub struct MpvControlImpl {
    player: MpvPlayer,
}

impl MpvControlImpl {
    pub fn new(config: &MpvConfig) -> Result<Self, libmpv2::Error> {
        let player = MpvPlayer::new(config)?;
        // v0.1.0: `MpvPlayer::new` applies `as_options`, which sets
        // `vo=libmpv` for the future embedded render path — but no
        // render context exists yet, so with `vo=libmpv` there would
        // be no window and no picture. The stage-1 options
        // (`MpvConfig::stage1_options`) deliberately set no `vo` key,
        // so override it here to mpv's own window (`gpu`) until the
        // embedded video surface lands; delete this override then.
        player.set_option("vo", "gpu")?;
        player.enable_log_messages()?;
        Ok(Self { player })
    }
}

fn map_err(e: libmpv2::Error) -> PlayerError {
    PlayerError::Mpv(e.to_string())
}

impl MpvControl for MpvControlImpl {
    fn load_pair(&mut self, video_url: &str, audio_url: Option<&str>) -> Result<(), PlayerError> {
        self.player.load_pair(video_url, audio_url).map_err(map_err)
    }

    fn set_pause(&mut self, pause: bool) -> Result<(), PlayerError> {
        self.player.set_pause(pause).map_err(map_err)
    }

    fn seek(&mut self, secs: f64) -> Result<(), PlayerError> {
        self.player.seek(secs).map_err(map_err)
    }

    fn set_option(&mut self, key: &str, value: &str) -> Result<(), PlayerError> {
        self.player.set_option(key, value).map_err(map_err)
    }

    fn load_subtitles(&mut self, path: &str) -> Result<(), PlayerError> {
        self.player.load_subtitles(path).map_err(map_err)
    }

    fn disable_subtitles(&mut self) -> Result<(), PlayerError> {
        self.player.disable_subtitles().map_err(map_err)
    }

    fn apply_stage1_options(&mut self, cache_dir: &Path) -> Result<(), PlayerError> {
        let cache_dir = cache_dir.to_string_lossy().into_owned();
        for (key, value) in MpvConfig::stage1_options(&cache_dir) {
            self.player.set_option(key, &value).map_err(map_err)?;
        }
        Ok(())
    }

    fn time_pos(&self) -> Option<f64> {
        self.player.time_pos()
    }

    fn duration(&self) -> Option<f64> {
        self.player.duration()
    }

    fn wait_event(&mut self, timeout: Duration) -> Option<MpvEvent> {
        let event = self.player.wait_event(timeout.as_secs_f64())?;
        match event {
            Ok(Event::EndFile(reason)) => Some(match reason {
                mpv_end_file_reason::Error => MpvEvent::EndFileError,
                mpv_end_file_reason::Eof => MpvEvent::EndFileEof,
                mpv_end_file_reason::Stop => MpvEvent::EndFileStopped,
                _ => MpvEvent::Other,
            }),
            Ok(Event::LogMessage { text, .. }) => {
                if text.contains("403") {
                    Some(MpvEvent::Log403)
                } else {
                    Some(MpvEvent::Other)
                }
            }
            Ok(Event::Seek) => Some(MpvEvent::Seek),
            Ok(Event::PlaybackRestart) => Some(MpvEvent::PlaybackRestart),
            Ok(_) => Some(MpvEvent::Other),
            Err(_) => None,
        }
    }
}
