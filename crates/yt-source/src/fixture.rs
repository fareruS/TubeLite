//! Fixture-backed source for offline development and tests.
//! Parses the JSON fixtures in `tests/fixtures/` — no network, ever.

use async_trait::async_trait;
use yt_model::{
    Capabilities, ChannelId, ChannelPage, Cursor, Format, Page, PlayableMedia, PlaylistId,
    QualityPref, VideoCard, VideoDetails, VideoId,
};

use crate::error::{Result, SourceError};
use crate::traits::{SearchKind, SearchQuery, StreamSource};

pub struct FixtureSource;

const SEARCH_FIXTURE: &str = include_str!("../tests/fixtures/search.json");
const VIDEO_FIXTURE: &str = include_str!("../tests/fixtures/video.json");
const CHANNEL_FIXTURE: &str = include_str!("../tests/fixtures/channel.json");
const PLAYLIST_FIXTURE: &str = include_str!("../tests/fixtures/playlist.json");

#[async_trait]
impl StreamSource for FixtureSource {
    fn name(&self) -> &'static str {
        "fixture"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            limitation_note: Some("Offline fixture source — demo data only.".into()),
            ..Capabilities::default()
        }
    }

    async fn search(&self, q: &SearchQuery) -> Result<Page<VideoCard>> {
        let _ = q.kind;
        let page: Page<VideoCard> =
            serde_json::from_str(SEARCH_FIXTURE).map_err(|e| SourceError::Parse(e.to_string()))?;
        Ok(page)
    }

    async fn video(&self, id: &VideoId) -> Result<VideoDetails> {
        let details: VideoDetails =
            serde_json::from_str(VIDEO_FIXTURE).map_err(|e| SourceError::Parse(e.to_string()))?;
        if details.id.0 != id.0 {
            return Err(SourceError::NotFound(id.0.clone()));
        }
        Ok(details)
    }

    async fn channel(&self, id: &ChannelId) -> Result<ChannelPage> {
        let page: ChannelPage =
            serde_json::from_str(CHANNEL_FIXTURE).map_err(|e| SourceError::Parse(e.to_string()))?;
        if page.id.0 != id.0 {
            return Err(SourceError::NotFound(id.0.clone()));
        }
        Ok(page)
    }

    async fn playlist(&self, id: &PlaylistId, _cursor: Option<Cursor>) -> Result<Page<VideoCard>> {
        let _ = id;
        let page: Page<VideoCard> = serde_json::from_str(PLAYLIST_FIXTURE)
            .map_err(|e| SourceError::Parse(e.to_string()))?;
        Ok(page)
    }

    async fn playable(&self, id: &VideoId, pref: QualityPref) -> Result<PlayableMedia> {
        // Fixture URLs are intentionally fake and unplayable; they exist so
        // format selection, expiry handling and quality switching can be
        // tested offline. Never hand a fixture URL to mpv in production.
        let _ = SearchKind::Videos;
        let height = pref.height();
        let video = height.map(|h| Format {
            itag: if h >= 1080 {
                248
            } else if h >= 720 {
                247
            } else {
                243
            },
            mime_type: "video/webm".into(),
            height: Some(h),
            bitrate: Some(2_500_000),
            url: format!(
                "https://example.invalid/googlevideo/video/{}/itag-{}",
                id.0, h
            ),
            expires_at_unix: Some(1_900_000_000),
        });
        Ok(PlayableMedia {
            video_id: id.clone(),
            video,
            audio: Some(Format {
                itag: 251,
                mime_type: "audio/webm".into(),
                height: None,
                bitrate: Some(160_000),
                url: format!("https://example.invalid/googlevideo/audio/{}", id.0),
                expires_at_unix: Some(1_900_000_000),
            }),
            candidates: vec![],
            expires_at_unix: Some(1_900_000_000),
        })
    }
}
