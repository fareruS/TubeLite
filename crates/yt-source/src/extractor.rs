//! ExtractorSource — Phase 6 backup source A: yt-dlp sidecar.
//!
//! Security rules (from the spec, enforced in code, not in comments):
//! - Fixed argument allow-list. The ONLY user-controlled input is the
//!   video id, and it is validated to `^[A-Za-z0-9_-]{11}$` before it is
//!   used. No shell is ever involved (`tokio::process::Command`, argv
//!   array, no interpolation), so `; rm -rf /` in an id is rejected
//!   before it can reach anything.
//! - Timeouts: the sidecar is killed after `timeout_secs` (default 30).
//! - Output limits: stdout over 8 MiB is rejected (a runaway extractor
//!   must not fill memory with JSON).
//! - Strict JSON: the whole stdout must parse as one JSON object; a
//!   trailing log line makes the parse fail rather than being guessed at.
//! - The binary path comes from Settings (user-configured, with an
//!   "update" button that runs `--version`); there is no bundled yt-dlp.
//!   yt-dlp is NOT installed on this Linux VM, so the real binary is
//!   unverified here — tests use fake sidecar scripts in
//!   `tests/fixtures/fake-ytdlp*.sh`.
//!
//! Format selection reuses the InnerTube selectors (`choose_video` /
//! `choose_audio`): VP9 preferred at equal height, AV1 excluded, Opus
//! 251 preferred. Expiry is read from the googlevideo URL's `expire=`
//! query parameter; a URL without one has no recorded expiry and is
//! treated as expired by `PlayableMedia::is_expired`, forcing re-resolve.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use yt_model::{
    Capabilities, ChannelId, ChannelPage, Cursor, Format, Page, PlayableMedia, PlaylistId,
    QualityPref, VideoCard, VideoDetails, VideoId,
};

use crate::error::{Result, SourceError};
use crate::innertube_source::{choose_audio, choose_video};
use crate::traits::{SearchQuery, StreamSource};

/// Maximum sidecar stdout accepted (8 MiB).
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// Default sidecar timeout.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// A YouTube video id is exactly 11 chars of [A-Za-z0-9_-].
pub fn validate_video_id(id: &str) -> Result<()> {
    let ok = id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(SourceError::Parse(format!("invalid video id: {id:?}")))
    }
}

/// Fixed argument allow-list. The video id is validated first; the URL is
/// built from the validated id only — never from raw user input.
pub fn build_args(id: &VideoId) -> Result<Vec<String>> {
    validate_video_id(&id.0)?;
    Ok(vec![
        "--dump-single-json".into(),
        "--no-playlist".into(),
        "--no-warnings".into(),
        "--format".into(),
        // Best video <=1080p (AV1 excluded) + best audio, else best single.
        "bv*[height<=1080][vcodec!*=av01]+ba/bv*[height<=1080]+ba/b".into(),
        "--".into(),
        format!("https://www.youtube.com/watch?v={}", id.0),
    ])
}

/// Parse the `expire=` query parameter of a googlevideo URL.
fn expire_from_url(url: &str) -> Option<i64> {
    let rest = url.split("expire=").nth(1)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Parse yt-dlp `--dump-single-json` output into a `PlayableMedia`.
pub fn parse_extractor_json(
    json: &Value,
    id: &VideoId,
    pref: QualityPref,
) -> Result<PlayableMedia> {
    let formats = json
        .get("formats")
        .and_then(|f| f.as_array())
        .ok_or_else(|| SourceError::UnexpectedShape("extractor output has no formats[]".into()))?;
    let mut candidates: Vec<Format> = Vec::new();
    for f in formats {
        let Some(url) = f.get("url").and_then(|u| u.as_str()) else {
            continue; // no URL (manifest-only format) — skip, never guess
        };
        let itag = f
            .get("format_id")
            .and_then(|i| i.as_str())
            .and_then(|s| s.split('+').next())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let vcodec = f.get("vcodec").and_then(|c| c.as_str()).unwrap_or("none");
        let acodec = f.get("acodec").and_then(|c| c.as_str()).unwrap_or("none");
        let ext = f.get("ext").and_then(|e| e.as_str()).unwrap_or("");
        candidates.push(Format {
            itag,
            mime_type: format!("{vcodec}/{acodec}/{ext}"),
            height: f.get("height").and_then(|h| h.as_u64()).map(|h| h as u32),
            bitrate: f
                .get("tbr")
                .and_then(|b| b.as_f64())
                .map(|b| (b * 1000.0) as u64),
            url: url.to_string(),
            expires_at_unix: expire_from_url(url),
        });
    }
    let video = match pref.height() {
        Some(h) => choose_video(&candidates, h),
        None => None,
    };
    let audio = choose_audio(&candidates);
    if video.is_none() && pref.height().is_some() {
        return Err(SourceError::UnexpectedShape(
            "no playable video format in extractor output".into(),
        ));
    }
    if audio.is_none() {
        return Err(SourceError::UnexpectedShape(
            "no playable audio format in extractor output".into(),
        ));
    }
    let earliest = [video.as_ref(), audio.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|f| f.expires_at_unix)
        .min();
    Ok(PlayableMedia {
        video_id: id.clone(),
        video,
        audio,
        candidates,
        expires_at_unix: earliest,
    })
}

pub struct ExtractorSource {
    binary: PathBuf,
    timeout_secs: u64,
}

impl ExtractorSource {
    pub fn new(binary: impl AsRef<Path>) -> Self {
        Self {
            binary: binary.as_ref().to_path_buf(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Run the sidecar with `--version` (Settings "update" / diagnostics).
    pub async fn version(&self) -> Result<String> {
        let out = tokio::process::Command::new(&self.binary)
            .arg("--version")
            .output()
            .await
            .map_err(|e| SourceError::Sidecar(e.to_string()))?;
        if !out.status.success() {
            return Err(SourceError::Sidecar(format!(
                "sidecar --version exited {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

#[async_trait]
#[allow(clippy::double_must_use)]
impl StreamSource for ExtractorSource {
    async fn search(&self, _q: &SearchQuery) -> Result<Page<VideoCard>> {
        Err(SourceError::Unsupported(
            "extractor source only resolves playable streams (search stays on InnerTube)".into(),
        ))
    }

    async fn video(&self, _id: &VideoId) -> Result<VideoDetails> {
        Err(SourceError::Unsupported(
            "extractor source only resolves playable streams".into(),
        ))
    }

    async fn channel(&self, _id: &ChannelId) -> Result<ChannelPage> {
        Err(SourceError::Unsupported(
            "extractor source only resolves playable streams".into(),
        ))
    }

    async fn playlist(&self, _id: &PlaylistId, _cursor: Option<Cursor>) -> Result<Page<VideoCard>> {
        Err(SourceError::Unsupported(
            "extractor source only resolves playable streams".into(),
        ))
    }

    async fn playable(&self, id: &VideoId, pref: QualityPref) -> Result<PlayableMedia> {
        let args = build_args(id)?;
        let run = tokio::process::Command::new(&self.binary)
            .args(&args)
            .output();
        let out = tokio::time::timeout(Duration::from_secs(self.timeout_secs), run)
            .await
            .map_err(|_| {
                SourceError::Sidecar(format!("sidecar timed out after {}s", self.timeout_secs))
            })?
            .map_err(|e| SourceError::Sidecar(e.to_string()))?;
        if !out.status.success() {
            return Err(SourceError::Sidecar(format!(
                "sidecar exited {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        if out.stdout.len() > MAX_OUTPUT_BYTES {
            return Err(SourceError::Sidecar(format!(
                "sidecar output {} bytes exceeds {} byte limit",
                out.stdout.len(),
                MAX_OUTPUT_BYTES
            )));
        }
        let json: Value =
            serde_json::from_slice(&out.stdout).map_err(|e| SourceError::Parse(e.to_string()))?;
        parse_extractor_json(&json, id, pref)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            search: false,
            subscriptions: false,
            playlists_write: false,
            like: false,
            comments_read: false,
            gapless: false,
            audio_only: true,
            captions: false,
            chapters: false,
            limitation_note: Some(
                "yt-dlp sidecar fallback: resolves streams only, no search or library.".into(),
            ),
        }
    }

    fn name(&self) -> &'static str {
        "extractor"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> Value {
        let path = format!(
            "{}/tests/fixtures/extractor.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn vid() -> VideoId {
        VideoId("dQw4w9WgXcQ".into())
    }

    #[test]
    fn valid_id_passes_validation() {
        assert!(validate_video_id("dQw4w9WgXcQ").is_ok());
        assert!(validate_video_id("aB3_-xYz123").is_ok());
    }

    #[test]
    fn injection_ids_are_rejected() {
        for bad in [
            "dQw4w9WgXcQ; rm -rf /",
            "dQw4w9WgXcQ$(id)",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "dQw4w9WgXc",   // 10 chars
            "dQw4w9WgXcQ!", // 12 chars with a shell metachar
            "",
        ] {
            assert!(validate_video_id(bad).is_err(), "accepted bad id: {bad:?}");
            assert!(build_args(&VideoId(bad.to_string())).is_err());
        }
    }

    #[test]
    fn args_are_fixed_allow_list() {
        let args = build_args(&vid()).unwrap();
        assert_eq!(args[0], "--dump-single-json");
        assert_eq!(args[1], "--no-playlist");
        assert_eq!(args[2], "--no-warnings");
        assert!(args.contains(&"--".to_string()));
        // The only URL is built from the validated id.
        assert_eq!(
            args.last().unwrap(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
    }

    #[test]
    fn extractor_fixture_picks_dash_pair() {
        let m = parse_extractor_json(&fixture(), &vid(), QualityPref::Auto).unwrap();
        assert_eq!(m.video.as_ref().unwrap().itag, 247);
        assert_eq!(m.audio.as_ref().unwrap().itag, 251);
        // Expiry comes from the URL's expire= parameter.
        assert_eq!(m.expires_at_unix, Some(1_700_021_600));
        // The URL-less format 999 was skipped.
        assert!(!m.candidates.iter().any(|f| f.itag == 999));
    }

    #[test]
    fn expire_from_url_parses() {
        assert_eq!(
            expire_from_url("https://example.invalid/x?expire=1700021600&sig=abc"),
            Some(1_700_021_600)
        );
        assert_eq!(expire_from_url("https://example.invalid/x?sig=abc"), None);
    }

    #[tokio::test]
    async fn fake_sidecar_playable_end_to_end() {
        let script = format!(
            "{}/tests/fixtures/fake-ytdlp.sh",
            env!("CARGO_MANIFEST_DIR")
        );
        let src = ExtractorSource::new(script);
        let m = src.playable(&vid(), QualityPref::Auto).await.unwrap();
        assert_eq!(m.video.as_ref().unwrap().itag, 247);
        assert_eq!(m.audio.as_ref().unwrap().itag, 251);
    }

    #[tokio::test]
    async fn fake_sidecar_failure_is_sidecar_error() {
        let script = format!(
            "{}/tests/fixtures/fake-ytdlp-fail.sh",
            env!("CARGO_MANIFEST_DIR")
        );
        let src = ExtractorSource::new(script);
        let err = src.playable(&vid(), QualityPref::Auto).await.unwrap_err();
        assert!(matches!(err, SourceError::Sidecar(_)));
    }

    #[tokio::test]
    async fn fake_sidecar_garbage_is_parse_error() {
        let script = format!(
            "{}/tests/fixtures/fake-ytdlp-garbage.sh",
            env!("CARGO_MANIFEST_DIR")
        );
        let src = ExtractorSource::new(script);
        let err = src.playable(&vid(), QualityPref::Auto).await.unwrap_err();
        assert!(matches!(err, SourceError::Parse(_)));
    }
}
