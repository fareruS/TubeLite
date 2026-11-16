//! mpv options, each with its reason. Applied by `mpv_backend.rs` when
//! the `mpv` feature is enabled; this struct itself is pure data so the
//! documented values are testable without libmpv.
//!
//! Measured values: NONE yet on this machine. The numbers below are the
//! brief's starting points, not measurements. Tune by measurement and
//! record before/after in docs/MEMORY.md + docs/UI-PERFORMANCE.md.

/// Documented mpv configuration (starting points from the brief).
#[derive(Debug, Clone)]
pub struct MpvConfig {
    /// App cache directory for `demuxer-cache-dir`.
    pub cache_dir: String,
    /// `demuxer-max-bytes` — bounds packet metadata, not audio bytes,
    /// when `cache-on-disk=yes`.
    pub demuxer_max_bytes: u64,
    pub demuxer_max_back_bytes: u64,
    /// `cache-pause-wait` — lowered for faster resume after seek.
    pub cache_pause_wait: f64,
    /// `hwdec=auto-safe`, or `no` when Settings forces software decode.
    pub hwdec: String,
    /// Loudness normalisation audio filter, off by default.
    pub loudness_normalisation: bool,
}

impl MpvConfig {
    pub fn video(cache_dir: impl Into<String>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            // Video needs a larger demuxer buffer than audio-only (64 MiB).
            // Starting point — measure 720p/1080p seeks and adjust.
            demuxer_max_bytes: 128 * 1024 * 1024,
            demuxer_max_back_bytes: 64 * 1024 * 1024,
            cache_pause_wait: 0.3,
            hwdec: "auto-safe".into(),
            loudness_normalisation: false,
        }
    }

    pub fn audio_only(cache_dir: impl Into<String>) -> Self {
        Self {
            demuxer_max_bytes: 64 * 1024 * 1024,
            demuxer_max_back_bytes: 64 * 1024 * 1024,
            ..Self::video(cache_dir)
        }
    }

    /// Options as (key, value) pairs, in the order they should be set.
    /// `ytdl=no`: resolution is done by our own layer, not mpv's script hook.
    /// `gapless-audio=yes` + `prefetch-playlist=yes`: audio-only queues.
    /// `vo=libmpv`: render API, never mpv's own window.
    pub fn as_options(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ytdl", "no".into()),
            ("vo", "libmpv".into()),
            ("hwdec", self.hwdec.clone()),
            ("vid", "auto".into()), // audio-only mode sets vid=no at runtime
            ("osd-level", "0".into()),
            ("osd-bar", "no".into()),
            ("gapless-audio", "yes".into()),
            ("prefetch-playlist", "yes".into()),
            ("demuxer", "lavf".into()),
            ("cache", "yes".into()),
            ("cache-on-disk", "yes".into()),
            ("demuxer-cache-dir", self.cache_dir.clone()),
            ("demuxer-max-bytes", self.demuxer_max_bytes.to_string()),
            ("demuxer-max-back-bytes", self.demuxer_max_back_bytes.to_string()),
            ("cache-pause-wait", self.cache_pause_wait.to_string()),
            (
                "stream-lavf-o",
                "reconnect=1,reconnect_streamed=1,reconnect_on_network_error=1,reconnect_delay_max=2,reconnect_max_retries=6,reconnect_delay_total_max=30,short_seek_size=1".into(),
            ),
        ]
    }

    /// Stage 1 options: exactly the set the stage-1 spec requires,
    /// applied before any playback. Note the 64 MiB demuxer caps
    /// (the older `as_options` uses 128 MiB for video as a starting
    /// point; stage 1 pins 64 MiB for both directions).
    pub fn stage1_options(cache_dir: &str) -> Vec<(&'static str, String)> {
        vec![
            ("ytdl", "no".into()),
            ("hwdec", "auto-safe".into()),
            ("cache", "yes".into()),
            ("cache-on-disk", "yes".into()),
            ("demuxer-cache-dir", cache_dir.to_string()),
            ("demuxer-max-bytes", (64 * 1024 * 1024).to_string()),
            ("demuxer-max-back-bytes", (64 * 1024 * 1024).to_string()),
            ("osd-level", "0".into()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_has_ytdl_no_and_disk_cache() {
        let opts = MpvConfig::video("/tmp/cache").as_options();
        let get = |k: &str| {
            opts.iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("ytdl").as_deref(), Some("no"));
        assert_eq!(get("cache-on-disk").as_deref(), Some("yes"));
        assert_eq!(get("hwdec").as_deref(), Some("auto-safe"));
        assert_eq!(get("cache-pause-wait").as_deref(), Some("0.3"));
    }

    #[test]
    fn stage1_options_match_the_spec() {
        let opts = MpvConfig::stage1_options("/tmp/app-cache");
        let get = |k: &str| {
            opts.iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("ytdl").as_deref(), Some("no"));
        assert_eq!(get("hwdec").as_deref(), Some("auto-safe"));
        assert_eq!(get("cache").as_deref(), Some("yes"));
        assert_eq!(get("cache-on-disk").as_deref(), Some("yes"));
        assert_eq!(get("demuxer-cache-dir").as_deref(), Some("/tmp/app-cache"));
        assert_eq!(get("demuxer-max-bytes").as_deref(), Some("67108864"));
        assert_eq!(get("demuxer-max-back-bytes").as_deref(), Some("67108864"));
        assert_eq!(get("osd-level").as_deref(), Some("0"));
    }
}
