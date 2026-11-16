//! Pure data types. No I/O, no Tauri, no mpv, no OS calls.
//! Every type here must be constructible and (de)serialisable offline
//! from a JSON fixture.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VideoId(pub String);
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(pub String);
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaylistId(pub String);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thumbnail {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

/// A card as shown in a list / feed. Small, already-shaped for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoCard {
    pub id: VideoId,
    pub title: String,
    pub channel_id: ChannelId,
    pub channel_title: String,
    pub thumbnails: Vec<Thumbnail>,
    /// Duration in seconds, if known.
    pub duration_secs: Option<u64>,
    pub view_count: Option<u64>,
    pub published_at: Option<String>,
    pub is_live: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub start_secs: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionTrack {
    pub language_code: String,
    pub language_name: String,
    /// URL template / base URL as returned by the source. Never logged in full.
    pub base_url: String,
    pub is_auto_generated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoDetails {
    pub id: VideoId,
    pub title: String,
    pub description: String,
    pub channel_id: ChannelId,
    pub channel_title: String,
    pub thumbnails: Vec<Thumbnail>,
    pub duration_secs: Option<u64>,
    pub view_count: Option<u64>,
    pub like_count: Option<u64>,
    pub published_at: Option<String>,
    pub chapters: Vec<Chapter>,
    pub captions: Vec<CaptionTrack>,
    pub related: Vec<VideoCard>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelPage {
    pub id: ChannelId,
    pub title: String,
    pub description: String,
    pub thumbnails: Vec<Thumbnail>,
    pub subscriber_count: Option<u64>,
    pub videos: Vec<VideoCard>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<Cursor>,
}

/// One adaptive stream. Video-only and audio-only are separate entries;
/// there is no single 1080p file with sound above 720p (DASH).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Format {
    /// YouTube itag, e.g. 251 (Opus audio), 248 (VP9 1080p), 137 (AVC 1080p).
    pub itag: u32,
    pub mime_type: String,
    /// Height for video, None for audio-only.
    pub height: Option<u32>,
    pub bitrate: Option<u64>,
    pub url: String,
    /// Unix epoch seconds when this URL expires. Never persist as permanent.
    pub expires_at_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayableMedia {
    pub video_id: VideoId,
    /// Chosen video format, None in audio-only mode.
    pub video: Option<Format>,
    /// Chosen audio format.
    pub audio: Option<Format>,
    /// All candidate formats, for quality switching without a fresh resolve
    /// when the source already returned them.
    pub candidates: Vec<Format>,
    /// Unix epoch seconds; the earliest expiry among the chosen URLs.
    pub expires_at_unix: Option<i64>,
}

impl PlayableMedia {
    /// True if any chosen URL is expired (or has no expiry recorded, in
    /// which case callers must re-resolve rather than assume it is fresh).
    pub fn is_expired(&self, now_unix: i64) -> bool {
        match self.expires_at_unix {
            Some(exp) => now_unix >= exp,
            None => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityPref {
    Auto,
    AudioOnly,
    Height(u32), // 144, 240, 360, 480, 720, 1080
}

impl QualityPref {
    pub fn height(self) -> Option<u32> {
        match self {
            QualityPref::Auto => Some(720),
            QualityPref::AudioOnly => None,
            QualityPref::Height(h) => Some(h),
        }
    }
}

/// What a source mode can and cannot do. The UI hides/disables what is
/// unavailable, with a tooltip — it never shows a broken button.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub search: bool,
    pub subscriptions: bool,
    pub playlists_write: bool,
    pub like: bool,
    pub comments_read: bool,
    pub gapless: bool,
    pub audio_only: bool,
    pub captions: bool,
    pub chapters: bool,
    /// Human-readable reason shown in Settings when a mode is limited.
    pub limitation_note: Option<String>,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            search: true,
            subscriptions: true,
            playlists_write: false,
            like: false,
            comments_read: true,
            gapless: true,
            audio_only: true,
            captions: true,
            chapters: true,
            limitation_note: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playable_expiry_is_checked() {
        let m = PlayableMedia {
            video_id: VideoId("dQw4w9WgXcQ".into()),
            video: None,
            audio: None,
            candidates: vec![],
            expires_at_unix: Some(1_700_000_000),
        };
        assert!(!m.is_expired(1_699_999_999));
        assert!(m.is_expired(1_700_000_000));
        // No expiry recorded => treat as expired, never as fresh.
        let m2 = PlayableMedia {
            expires_at_unix: None,
            ..m
        };
        assert!(m2.is_expired(0));
    }

    #[test]
    fn quality_pref_height() {
        assert_eq!(QualityPref::Auto.height(), Some(720));
        assert_eq!(QualityPref::AudioOnly.height(), None);
        assert_eq!(QualityPref::Height(1080).height(), Some(1080));
    }
}
