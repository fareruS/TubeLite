//! URL resolution: turn a YouTube video id into direct stream URLs.
//!
//! Stage 1 uses yt-dlp as the resolver (the InnerTube resolver is a
//! later stage; this module does not touch InnerTube at all).
//!
//! - [`Resolver`]: the trait the [`Player`](crate::Player) resolves
//!   through. Swap it for a fake in tests, or for an InnerTube-based
//!   resolver later, without touching playback.
//! - [`YtDlpResolver`]: spawns `yt-dlp --no-playlist
//!   --dump-single-json`, picks the best video format at or below the
//!   requested height and the best audio-only format.
//! - Cookie handling: an optional Netscape cookie export is written to
//!   a temp file with mode 0600 and removed by a [`Drop`] guard. Cookie
//!   values never reach an error message or a log line —
//!   [`redact_secrets`] scrubs them from yt-dlp's stderr, and a test
//!   asserts that.

use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::OnceCell;

/// A resolved video: direct stream URLs plus the metadata playback
/// needs. `audio_url` is `None` when the chosen video format already
/// carries audio (a combined format), or when no audio-only format
/// exists.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    pub video_url: String,
    pub audio_url: Option<String>,
    /// Request headers for the stream URLs (from yt-dlp's
    /// `http_headers`, e.g. `User-Agent`).
    pub headers: Vec<(String, String)>,
    /// When the video URL expires, parsed from its `expire=` query
    /// parameter. `None` when the URL carries no expiry.
    pub expires_at: Option<SystemTime>,
    pub title: String,
    /// Duration in seconds, when yt-dlp reports one.
    pub duration: Option<f64>,
    /// yt-dlp's dump has no tracking/ping URL; always `None` for now.
    pub tracking_url: Option<String>,
}

/// Errors from resolution. No variant carries a cookie value.
#[derive(Debug, Clone, Error)]
pub enum ResolveError {
    #[error("invalid video id {0:?}")]
    InvalidVideoId(String),

    #[error("yt-dlp not found ({detail}). Install it with: {update_command}")]
    YtDlpMissing {
        detail: String,
        update_command: String,
    },

    #[error("yt-dlp is outdated (version {version}). Update it with: {update_command}")]
    YtDlpOutdated {
        version: String,
        update_command: String,
    },

    #[error("yt-dlp failed for {video_id}: {stderr}")]
    YtDlpFailed { video_id: String, stderr: String },

    #[error("yt-dlp output could not be parsed: {0}")]
    Parse(String),

    #[error("no video format at or below {max_height}p for {video_id}")]
    NoSuitableFormat { video_id: String, max_height: u32 },

    #[error("resolution timed out for {0}")]
    Timeout(String),

    #[error("io error: {0}")]
    Io(String),
}

impl From<std::io::Error> for ResolveError {
    fn from(e: std::io::Error) -> Self {
        ResolveError::Io(e.to_string())
    }
}

/// Resolve a video id into direct stream URLs.
///
/// The method returns a boxed future (rather than `async fn`) so the
/// trait stays object-safe for `Arc<dyn Resolver>`.
pub trait Resolver: Send + Sync {
    fn resolve(
        &self,
        video_id: &str,
        max_height: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Resolved, ResolveError>> + Send + '_>>;
}

/// How to install/update yt-dlp, shown in the missing/outdated errors.
pub const YT_DLP_UPDATE_COMMAND: &str = "yt-dlp -U  (or: pip install -U yt-dlp)";

/// Policy floor for the yt-dlp version (date-based versions,
/// `YYYY.MM.DD`). Older builds predate fixes the resolver relies on;
/// the floor is a policy constant, not a measurement.
const MIN_YT_DLP_VERSION: (u32, u32, u32) = (2025, 1, 1);

/// A video id is 1–64 chars from the YouTube id alphabet, and never
/// starts with `-` (a leading dash could be read as a flag).
fn valid_video_id(video_id: &str) -> bool {
    !video_id.is_empty()
        && !video_id.starts_with('-')
        && video_id.len() <= 64
        && video_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A cookie file written to a temp path with mode 0600, removed when
/// dropped. The file is created with the restrictive mode *before*
/// the cookie content is written, so there is no window where the
/// file exists with default permissions.
///
/// On non-Unix platforms the mode cannot be tightened the same way;
/// the file is still created in the OS temp dir and still removed on
/// drop.
pub(crate) struct CookieTempFile {
    path: PathBuf,
}

static COOKIE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl CookieTempFile {
    pub fn new(cookies: &str) -> std::io::Result<Self> {
        let unique = COOKIE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "tubelite-cookies-{}-{}-{}.txt",
            std::process::id(),
            unique,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        );
        let path = std::env::temp_dir().join(name);
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            let _ = fs::set_permissions(&path, perms);
        }
        file.write_all(cookies.as_bytes())?;
        file.sync_all()?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for CookieTempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Scrub cookie values out of text (yt-dlp stderr, error messages).
/// Every `secret` is replaced with `[redacted]`; the replacement is
/// also applied to `Cookie:` header lines as a whole, so a secret
/// that arrives in a different shape is still caught.
pub fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret, "[redacted]");
        }
    }
    // Anything that still looks like a Cookie header value is redacted
    // wholesale (header name kept, value dropped).
    let mut redacted = String::with_capacity(out.len());
    for line in out.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("Cookie:") {
            let newline = if rest.ends_with('\n') { "\n" } else { "" };
            redacted.push_str("Cookie:[redacted]");
            redacted.push_str(newline);
        } else {
            redacted.push_str(line);
        }
    }
    redacted
}

/// Parse a `YYYY.MM.DD` yt-dlp version string. Returns `None` when the
/// string does not look like a date version.
fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let version = version.trim();
    let mut parts = version.split('.');
    let year: u32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

/// One yt-dlp format entry (only the fields selection needs).
#[derive(Debug, Clone)]
struct Format {
    url: String,
    height: Option<u32>,
    vcodec: String,
    acodec: String,
    fps: Option<f64>,
    bitrate: Option<f64>,
}

fn format_from_json(value: &serde_json::Value) -> Option<Format> {
    let url = value.get("url")?.as_str()?.to_string();
    if url.is_empty() {
        return None;
    }
    let bitrate = value
        .get("tbr")
        .and_then(|v| v.as_f64())
        .or_else(|| value.get("abr").and_then(|v| v.as_f64()))
        .or_else(|| value.get("vbr").and_then(|v| v.as_f64()));
    Some(Format {
        url,
        height: value
            .get("height")
            .and_then(|v| v.as_u64())
            .map(|h| h as u32),
        vcodec: value
            .get("vcodec")
            .and_then(|v| v.as_str())
            .unwrap_or("none")
            .to_string(),
        acodec: value
            .get("acodec")
            .and_then(|v| v.as_str())
            .unwrap_or("none")
            .to_string(),
        fps: value.get("fps").and_then(|v| v.as_f64()),
        bitrate,
    })
}

/// Pick the best video format at or below `max_height` and the best
/// audio-only format, from a parsed `--dump-single-json` document.
///
/// - Video: video-only formats (`acodec == "none"`) first, then
///   combined formats as a fallback (their audio is used and no
///   separate audio URL is needed). Highest height wins, then fps,
///   then bitrate. A format with no height is treated as height 0
///   (it never wins over a sized one).
/// - Audio: audio-only formats (`vcodec == "none"`), highest bitrate
///   wins.
/// - Returns an error when no video format is at or below
///   `max_height`.
fn select_formats(
    doc: &serde_json::Value,
    video_id: &str,
    max_height: u32,
) -> Result<(Format, Option<Format>), ResolveError> {
    let formats = doc
        .get("formats")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let parsed: Vec<Format> = formats.iter().filter_map(format_from_json).collect();
    let is_video = |f: &Format| f.vcodec != "none";
    let is_audio = |f: &Format| f.acodec != "none";
    let height_of = |f: &Format| f.height.unwrap_or(0);

    // Video-only candidates at or below the cap, best first.
    let mut video_only: Vec<&Format> = parsed
        .iter()
        .filter(|f| is_video(f) && !is_audio(f) && height_of(f) <= max_height)
        .collect();
    video_only.sort_by(|a, b| {
        height_of(b)
            .cmp(&height_of(a))
            .then(
                b.fps
                    .partial_cmp(&a.fps)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                b.bitrate
                    .partial_cmp(&a.bitrate)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    let video = video_only.first().copied().cloned();

    // Combined formats as a fallback when no video-only format fits.
    let video = match video {
        Some(f) => f,
        None => {
            let mut combined: Vec<&Format> = parsed
                .iter()
                .filter(|f| is_video(f) && is_audio(f) && height_of(f) <= max_height)
                .collect();
            combined.sort_by(|a, b| {
                height_of(b)
                    .cmp(&height_of(a))
                    .then(
                        b.fps
                            .partial_cmp(&a.fps)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
                    .then(
                        b.bitrate
                            .partial_cmp(&a.bitrate)
                            .unwrap_or(std::cmp::Ordering::Equal),
                    )
            });
            match combined.first() {
                Some(f) => (*f).clone(),
                None => {
                    return Err(ResolveError::NoSuitableFormat {
                        video_id: video_id.to_string(),
                        max_height,
                    })
                }
            }
        }
    };

    // Audio: only needed when the video format has no audio track.
    let audio = if is_audio(&video) {
        None
    } else {
        let mut audio_only: Vec<&Format> = parsed
            .iter()
            .filter(|f| !is_video(f) && is_audio(f))
            .collect();
        audio_only.sort_by(|a, b| {
            b.bitrate
                .partial_cmp(&a.bitrate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        audio_only.first().copied().cloned()
    };

    Ok((video, audio))
}

/// Build a [`Resolved`] from a parsed dump and the chosen formats.
fn build_resolved(doc: &serde_json::Value, video: Format, audio: Option<Format>) -> Resolved {
    let headers = doc
        .get("http_headers")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let expires_at = url::Url::parse(&video.url)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "expire")
                .and_then(|(_, v)| v.parse::<u64>().ok())
        })
        .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)));
    Resolved {
        video_url: video.url,
        audio_url: audio.map(|f| f.url),
        headers,
        expires_at,
        title: doc
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        duration: doc.get("duration").and_then(|v| v.as_f64()),
        tracking_url: None,
    }
}

/// Resolve through yt-dlp.
///
/// `cookies` is the optional Netscape cookie export text. It is
/// written to a temp file (mode 0600, removed on drop) and passed to
/// yt-dlp with `--cookies`; the file path, never the content, reaches
/// the command line.
pub struct YtDlpResolver {
    yt_dlp: PathBuf,
    cookies: Option<String>,
    timeout: Duration,
    /// Cached result of the one-time version check.
    version_check: OnceCell<Result<(), ResolveError>>,
}

impl YtDlpResolver {
    pub fn new(yt_dlp: impl Into<PathBuf>) -> Self {
        Self {
            yt_dlp: yt_dlp.into(),
            cookies: None,
            timeout: Duration::from_secs(60),
            version_check: OnceCell::new(),
        }
    }

    pub fn with_cookies(mut self, cookies: impl Into<String>) -> Self {
        self.cookies = Some(cookies.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The cookie values to scrub from any surfaced text: every value
    /// from the Netscape export (the `name=value` pairs).
    fn cookie_values(&self) -> Vec<String> {
        let Some(cookies) = &self.cookies else {
            return Vec::new();
        };
        cookies
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .filter_map(|line| {
                let fields: Vec<&str> = line.split('\t').collect();
                fields.get(6).map(|v| v.to_string())
            })
            .filter(|v| !v.is_empty())
            .collect()
    }

    /// One-time version check: missing binary or a version older than
    /// [`MIN_YT_DLP_VERSION`] fails with the update command. The
    /// result is cached for the resolver's lifetime.
    async fn ensure_version(&self) -> Result<(), ResolveError> {
        self.version_check
            .get_or_init(|| async {
                let output = Command::new(&self.yt_dlp).arg("--version").output().await;
                match output {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        Err(ResolveError::YtDlpMissing {
                            detail: format!("{}: {e}", self.yt_dlp.display()),
                            update_command: YT_DLP_UPDATE_COMMAND.to_string(),
                        })
                    }
                    Err(e) => Err(ResolveError::Io(e.to_string())),
                    Ok(output) => {
                        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        match parse_version(&version) {
                            Some(v) if v >= MIN_YT_DLP_VERSION => Ok(()),
                            Some(_) => Err(ResolveError::YtDlpOutdated {
                                version,
                                update_command: YT_DLP_UPDATE_COMMAND.to_string(),
                            }),
                            None => Err(ResolveError::YtDlpOutdated {
                                version,
                                update_command: YT_DLP_UPDATE_COMMAND.to_string(),
                            }),
                        }
                    }
                }
            })
            .await
            .clone()
    }
}

impl Resolver for YtDlpResolver {
    fn resolve(
        &self,
        video_id: &str,
        max_height: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Resolved, ResolveError>> + Send + '_>> {
        let video_id = video_id.to_string();
        Box::pin(async move { self.resolve_inner(&video_id, max_height).await })
    }
}

impl YtDlpResolver {
    async fn resolve_inner(
        &self,
        video_id: &str,
        max_height: u32,
    ) -> Result<Resolved, ResolveError> {
        if !valid_video_id(video_id) {
            return Err(ResolveError::InvalidVideoId(video_id.to_string()));
        }
        self.ensure_version().await?;

        // The cookie file lives only for the duration of this call.
        let cookie_file = match &self.cookies {
            Some(cookies) => Some(CookieTempFile::new(cookies)?),
            None => None,
        };
        let url = format!("https://www.youtube.com/watch?v={video_id}");
        let mut cmd = Command::new(&self.yt_dlp);
        cmd.kill_on_drop(true)
            .arg("--no-playlist")
            .arg("--dump-single-json")
            .arg("--no-warnings")
            .arg(&url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(file) = &cookie_file {
            cmd.arg("--cookies").arg(file.path());
        }
        let child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ResolveError::YtDlpMissing {
                    detail: format!("{}: {e}", self.yt_dlp.display()),
                    update_command: YT_DLP_UPDATE_COMMAND.to_string(),
                }
            } else {
                ResolveError::Io(e.to_string())
            }
        })?;
        // `kill_on_drop(true)` above means a timeout kills the child:
        // dropping the `wait_with_output` future drops the `Child`.
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| ResolveError::Timeout(video_id.to_string()))?
            .map_err(|e| ResolveError::Io(e.to_string()))?;
        // The temp file is dropped here, at the end of the call.
        drop(cookie_file);

        let owned_secrets = self.cookie_values();
        let secret_refs: Vec<&str> = owned_secrets.iter().map(|s| s.as_str()).collect();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let stderr = redact_secrets(&stderr, &secret_refs);
            return Err(ResolveError::YtDlpFailed {
                video_id: video_id.to_string(),
                stderr,
            });
        }
        let doc: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| ResolveError::Parse(e.to_string()))?;
        let (video, audio) = select_formats(&doc, video_id, max_height)?;
        Ok(build_resolved(&doc, video, audio))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic yt-dlp --dump-single-json samples (see
    /// tests/fixtures/ytdlp-sample*.json): a few formats with
    /// height/fps/vcodec/acodec/url + title/duration, shaped like the
    /// real schema. Used only for format-selection tests.
    fn sample_doc() -> serde_json::Value {
        let text = include_str!("../tests/fixtures/ytdlp-sample.json");
        serde_json::from_str(text).expect("parse synthetic sample")
    }

    #[test]
    fn selects_best_video_at_or_below_cap_and_best_audio() {
        let doc = sample_doc();
        let (video, audio) = select_formats(&doc, "dQw4w9WgXcQ", 720).unwrap();
        assert_eq!(video.height, Some(720));
        assert_eq!(video.url, "https://example.com/v720.mp4?expire=1700000000");
        let audio = audio.expect("audio format");
        assert_eq!(audio.url, "https://example.com/a192.m4a?expire=1700000000");
        assert_eq!(audio.bitrate, Some(192.0));
    }

    #[test]
    fn cap_1080_picks_1080_and_cap_144_picks_144() {
        let doc = sample_doc();
        let (video, _) = select_formats(&doc, "dQw4w9WgXcQ", 1080).unwrap();
        assert_eq!(video.height, Some(1080));
        let (video, _) = select_formats(&doc, "dQw4w9WgXcQ", 144).unwrap();
        assert_eq!(video.height, Some(144));
    }

    #[test]
    fn no_format_at_or_below_cap_is_an_error() {
        let doc = sample_doc();
        let err = select_formats(&doc, "dQw4w9WgXcQ", 100).unwrap_err();
        assert!(matches!(
            err,
            ResolveError::NoSuitableFormat {
                max_height: 100,
                ..
            }
        ));
    }

    #[test]
    fn combined_format_fallback_carries_audio_and_needs_no_audio_url() {
        let text = include_str!("../tests/fixtures/ytdlp-sample-combined-only.json");
        let doc: serde_json::Value = serde_json::from_str(text).unwrap();
        let (video, audio) = select_formats(&doc, "dQw4w9WgXcQ", 720).unwrap();
        assert_eq!(
            video.url,
            "https://example.com/combined720.mp4?expire=1700000000"
        );
        assert!(audio.is_none());
        let resolved = build_resolved(&doc, video, audio);
        assert_eq!(resolved.audio_url, None);
        assert_eq!(resolved.title, "Combined only");
        assert_eq!(resolved.duration, Some(213.0));
    }

    #[test]
    fn expires_at_parsed_from_video_url() {
        let doc = sample_doc();
        let (video, audio) = select_formats(&doc, "dQw4w9WgXcQ", 720).unwrap();
        let resolved = build_resolved(&doc, video, audio);
        assert_eq!(
            resolved.expires_at,
            Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        );
        assert_eq!(resolved.headers.len(), 2);
    }

    #[test]
    fn video_id_validation() {
        assert!(valid_video_id("dQw4w9WgXcQ"));
        assert!(valid_video_id("a-b_c123456"));
        assert!(!valid_video_id(""));
        assert!(!valid_video_id("--dump-single-json"));
        assert!(!valid_video_id("id with spaces"));
        assert!(!valid_video_id("a/b"));
    }

    #[test]
    fn cookie_temp_file_is_0600_and_removed_on_drop() {
        let path = {
            let file = CookieTempFile::new("cookie-content").unwrap();
            let path = file.path().to_path_buf();
            assert!(path.exists());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "cookie temp file must be mode 0600");
            }
            path
        };
        assert!(!path.exists(), "cookie temp file must be removed on drop");
    }

    #[test]
    fn cookie_temp_files_are_unique() {
        let a = CookieTempFile::new("a").unwrap();
        let b = CookieTempFile::new("b").unwrap();
        assert_ne!(a.path(), b.path());
    }

    #[test]
    fn redact_secrets_never_leaks_cookie_values() {
        let stderr = "ERROR: got 403 with Cookie: SAPISID=secret-value-123; SID=other-secret\nsecond line SAPISID=secret-value-123 again";
        let redacted = redact_secrets(stderr, &["secret-value-123", "other-secret"]);
        assert!(!redacted.contains("secret-value-123"));
        assert!(!redacted.contains("other-secret"));
        assert!(redacted.contains("[redacted]"));
        assert!(redacted.contains("second line"));
    }

    #[test]
    fn version_parsing_and_floor() {
        assert_eq!(parse_version("2025.01.01"), Some((2025, 1, 1)));
        assert_eq!(parse_version("2024.10.07\n"), Some((2024, 10, 7)));
        assert_eq!(parse_version("garbage"), None);
        assert!(parse_version("2025.06.01") >= Some(MIN_YT_DLP_VERSION));
        assert!(parse_version("2024.12.31") < Some(MIN_YT_DLP_VERSION));
    }
}
