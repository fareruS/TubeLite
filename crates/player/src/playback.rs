//! Playback state machine — pure Rust, no mpv, no I/O.
//!
//! Phase 2 core: speed, seek, chapters, subtitles, resume, quality
//! switching, and URL-expiry / 403 handling. The mpv backend only applies
//! the state this machine computes; the rules are tested here, offline.
//!
//! Invariants:
//! - Quality switching preserves position, pause state, volume, speed and
//!   subtitle choice. It never restarts from 0:00. The <500 ms switch gap
//!   is a measured target, not something this module can assert.
//! - Speed is clamped to 0.25–3x. Seeks clamp to [0, duration].
//! - Resume only when the saved position is meaningfully mid-video.

use yt_model::Chapter;

/// What to do when a stream URL fails during playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlFailureAction {
    /// Re-resolve the same video (fresh URLs), then resume at the position.
    ReResolve,
    /// Try the next client in the fallback chain, then re-resolve.
    NextClientThenReResolve,
    /// Give up and surface a friendly error.
    Fail,
}

#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
    pub paused: bool,
    /// 0.25–3.0, pitch-corrected by mpv (scaletempo).
    pub speed: f64,
    /// 0–100.
    pub volume: f64,
    pub muted: bool,
    pub audio_only: bool,
    /// Current video height, None in audio-only mode.
    pub quality_height: Option<u32>,
    pub subtitle_lang: Option<String>,
    pub chapters: Vec<Chapter>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            position_secs: 0.0,
            duration_secs: None,
            paused: false,
            speed: 1.0,
            volume: 100.0,
            muted: false,
            audio_only: false,
            quality_height: Some(720),
            subtitle_lang: None,
            chapters: vec![],
        }
    }
}

impl PlaybackState {
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.25, 3.0);
    }

    pub fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 100.0);
    }

    /// Seek to an absolute position, clamped to the known duration.
    pub fn seek_to(&mut self, secs: f64) {
        let clamped = secs.max(0.0);
        self.position_secs = match self.duration_secs {
            Some(d) => clamped.min(d),
            None => clamped,
        };
    }

    pub fn seek_by(&mut self, delta_secs: f64) {
        self.seek_to(self.position_secs + delta_secs);
    }

    /// A quality switch: same position, pause, volume, speed, subtitle.
    /// Returns the position playback must resume at.
    pub fn quality_switch(&mut self, new_height: Option<u32>) -> f64 {
        self.quality_height = new_height;
        self.audio_only = new_height.is_none();
        self.position_secs
    }

    /// Toggle audio-only: drop the video track, keep everything else.
    /// Switching back restores the picture at the same position.
    pub fn set_audio_only(&mut self, audio_only: bool) -> f64 {
        self.audio_only = audio_only;
        if audio_only {
            self.quality_height = None;
        } else if self.quality_height.is_none() {
            self.quality_height = Some(720);
        }
        self.position_secs
    }

    /// Index of the chapter containing the current position, if any.
    pub fn current_chapter(&self) -> Option<usize> {
        if self.chapters.is_empty() {
            return None;
        }
        let mut idx = 0;
        for (i, ch) in self.chapters.iter().enumerate() {
            if ch.start_secs <= self.position_secs {
                idx = i;
            } else {
                break;
            }
        }
        Some(idx)
    }

    pub fn next_chapter_start(&self) -> Option<f64> {
        self.chapters
            .iter()
            .map(|c| c.start_secs)
            .find(|&s| s > self.position_secs)
    }

    pub fn prev_chapter_start(&self) -> Option<f64> {
        // Jump to the start of the current chapter, or the previous one if
        // we are within 3 s of the current chapter's start.
        let cur = self.current_chapter()?;
        let cur_start = self.chapters[cur].start_secs;
        if self.position_secs - cur_start > 3.0 {
            Some(cur_start)
        } else if cur > 0 {
            Some(self.chapters[cur - 1].start_secs)
        } else {
            Some(0.0)
        }
    }

    /// Decide whether a saved resume position is worth using.
    /// Skip it when it is at the very start (<5 s) or within 10 s of the end.
    pub fn should_resume(saved_secs: f64, duration_secs: Option<f64>) -> bool {
        if saved_secs < 5.0 {
            return false;
        }
        match duration_secs {
            Some(d) => saved_secs < d - 10.0,
            None => true,
        }
    }

    /// Classify a playback URL failure. Expired / 403 URLs are re-resolved,
    /// never misreported as "unsupported format". After two failed
    /// re-resolves for the same video, move to the next client.
    pub fn on_url_failure(expired_or_403: bool, reresolve_attempts: u32) -> UrlFailureAction {
        if !expired_or_403 {
            return UrlFailureAction::Fail;
        }
        if reresolve_attempts >= 2 {
            UrlFailureAction::NextClientThenReResolve
        } else {
            UrlFailureAction::ReResolve
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapters() -> Vec<Chapter> {
        vec![
            Chapter {
                title: "Intro".into(),
                start_secs: 0.0,
            },
            Chapter {
                title: "Middle".into(),
                start_secs: 60.0,
            },
            Chapter {
                title: "End".into(),
                start_secs: 180.0,
            },
        ]
    }

    #[test]
    fn speed_and_volume_clamp() {
        let mut s = PlaybackState::default();
        s.set_speed(0.1);
        assert_eq!(s.speed, 0.25);
        s.set_speed(9.0);
        assert_eq!(s.speed, 3.0);
        s.set_volume(150.0);
        assert_eq!(s.volume, 100.0);
        s.set_volume(-5.0);
        assert_eq!(s.volume, 0.0);
    }

    #[test]
    fn seek_clamps_to_duration() {
        let mut s = PlaybackState {
            duration_secs: Some(213.0),
            position_secs: 10.0,
            ..Default::default()
        };
        s.seek_to(999.0);
        assert_eq!(s.position_secs, 213.0);
        s.seek_by(-500.0);
        assert_eq!(s.position_secs, 0.0);
    }

    #[test]
    fn quality_switch_preserves_everything() {
        let mut s = PlaybackState {
            position_secs: 123.5,
            paused: true,
            speed: 1.5,
            volume: 42.0,
            subtitle_lang: Some("en".into()),
            ..Default::default()
        };
        let resume_at = s.quality_switch(Some(1080));
        assert_eq!(resume_at, 123.5);
        assert!(s.paused);
        assert_eq!(s.speed, 1.5);
        assert_eq!(s.volume, 42.0);
        assert_eq!(s.subtitle_lang.as_deref(), Some("en"));
        assert_eq!(s.quality_height, Some(1080));
        // Audio-only and back: same position.
        assert_eq!(s.set_audio_only(true), 123.5);
        assert!(s.audio_only);
        assert_eq!(s.set_audio_only(false), 123.5);
        assert!(!s.audio_only);
    }

    #[test]
    fn chapter_navigation() {
        let mut s = PlaybackState {
            position_secs: 70.0,
            chapters: chapters(),
            ..Default::default()
        };
        assert_eq!(s.current_chapter(), Some(1));
        assert_eq!(s.next_chapter_start(), Some(180.0));
        // 10 s into chapter 1 -> back to its start.
        assert_eq!(s.prev_chapter_start(), Some(60.0));
        // 1 s into chapter 1 -> back to previous chapter.
        s.position_secs = 61.0;
        assert_eq!(s.prev_chapter_start(), Some(0.0));
        s.position_secs = 200.0;
        assert_eq!(s.next_chapter_start(), None);
    }

    #[test]
    fn resume_rules() {
        assert!(!PlaybackState::should_resume(2.0, Some(213.0)));
        assert!(!PlaybackState::should_resume(208.0, Some(213.0)));
        assert!(PlaybackState::should_resume(100.0, Some(213.0)));
        assert!(PlaybackState::should_resume(100.0, None));
    }

    #[test]
    fn url_failure_classification() {
        assert_eq!(
            PlaybackState::on_url_failure(true, 0),
            UrlFailureAction::ReResolve
        );
        assert_eq!(
            PlaybackState::on_url_failure(true, 2),
            UrlFailureAction::NextClientThenReResolve
        );
        assert_eq!(
            PlaybackState::on_url_failure(false, 0),
            UrlFailureAction::Fail
        );
    }
}
