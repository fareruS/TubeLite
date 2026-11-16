//! Pure parsers for raw InnerTube responses — no I/O, no clock.
//!
//! Shapes are the ones YouTube actually returns (verified against the
//! public InnerTube schema as used by yt-dlp / NewPipe, and recorded in
//! `tests/fixtures/search_raw.json` and `player.json`). Unknown renderer
//! types are SKIPPED, never fatal — the response shape changes often and
//! a new ad/shelf renderer must not break the whole page.
//!
//! Best-effort fields: view counts ("1,234,567 views" / "12 watching"),
//! durations ("3:33" / "1:02:03"), published times ("2 weeks ago").
//! When a field is absent or unparsable, the typed field is None — the
//! parser never guesses a number.

use yt_model::{
    CaptionTrack, ChannelId, ChannelPage, Chapter, Cursor, Page, Thumbnail, VideoCard,
    VideoDetails, VideoId,
};

use crate::error::{Result, SourceError};

/// Concatenate a `{"runs": [{"text": ...}]}` or `{"simpleText": ...}`.
pub fn runs_text(v: &Value) -> String {
    if let Some(s) = v.get("simpleText").and_then(|s| s.as_str()) {
        return s.to_string();
    }
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                .collect::<String>()
        })
        .unwrap_or_default()
}

use serde_json::Value;

/// "1,234,567 views" -> 1234567; "12 watching" -> 12; "1.2M subscribers"
/// -> None (abbreviated counts are not exact numbers — never guess one).
pub fn parse_count(s: &str) -> Option<u64> {
    let digits: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .collect();
    if digits.is_empty() {
        return None;
    }
    // The digits must be the whole leading token: "1.2M" must not parse as 1.
    match s[digits.len()..].chars().next() {
        None | Some(' ') => digits.replace(',', "").parse().ok(),
        Some(_) => None,
    }
}

/// "3:33" -> 213; "1:02:03" -> 3723; unparsable -> None.
pub fn parse_duration(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    let mut total: u64 = 0;
    for p in parts {
        total = total.checked_mul(60)?.checked_add(p.parse::<u64>().ok()?)?;
    }
    Some(total)
}

/// Read a `{"thumbnails": [...]}` list directly.
fn thumbnail_list(v: &Value) -> Vec<Thumbnail> {
    v.get("thumbnails")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    Some(Thumbnail {
                        url: t.get("url")?.as_str()?.to_string(),
                        width: t.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32,
                        height: t.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as u32,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Read thumbnails from a renderer (`{"thumbnail": {"thumbnails": [...]}}`).
fn thumbnails(v: &Value) -> Vec<Thumbnail> {
    v.get("thumbnail").map(thumbnail_list).unwrap_or_default()
}

/// `default_channel_title` fills in when the renderer has no `ownerText`
/// (grid / playlist renderers on a channel or playlist page, where the
/// channel is known from the page header).
fn video_card_from_renderer(vr: &Value, default_channel_title: &str) -> Option<VideoCard> {
    let id = vr.get("videoId")?.as_str()?;
    let title = runs_text(vr.get("title").unwrap_or(&Value::Null));
    if title.is_empty() {
        return None;
    }
    let owner = runs_text(vr.get("ownerText").unwrap_or(&Value::Null));
    let channel_title = if owner.is_empty() {
        default_channel_title.to_string()
    } else {
        owner
    };
    let view_text = runs_text(vr.get("viewCountText").unwrap_or(&Value::Null));
    let is_live = view_text.contains("watching")
        || vr
            .get("badges")
            .and_then(|b| b.as_array())
            .map(|badges| {
                badges.iter().any(|b| {
                    b.get("metadataBadgeRenderer")
                        .and_then(|m| m.get("label"))
                        .and_then(|l| l.as_str())
                        == Some("LIVE")
                })
            })
            .unwrap_or(false);
    Some(VideoCard {
        id: VideoId(id.to_string()),
        title,
        channel_id: ChannelId(
            vr.get("channelId")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
        ),
        channel_title,
        thumbnails: thumbnails(vr),
        duration_secs: vr
            .get("lengthText")
            .map(runs_text)
            .and_then(|s| parse_duration(&s)),
        view_count: parse_count(&view_text),
        published_at: vr
            .get("publishedTimeText")
            .map(runs_text)
            .filter(|s| !s.is_empty()),
        is_live,
    })
}

/// Parse a raw `/search` response. Video cards only — channel and playlist
/// renderers are recognised and skipped (their pages are separate
/// Phase 3 work); unknown renderers are skipped too.
pub fn parse_search_response(json: &Value) -> Result<Page<VideoCard>> {
    let contents = json
        .get("contents")
        .and_then(|c| c.get("twoColumnSearchResultsRenderer"))
        .and_then(|r| r.get("primaryContents"))
        .and_then(|p| p.get("sectionListRenderer"))
        .and_then(|s| s.get("contents"))
        .and_then(|c| c.as_array())
        .ok_or_else(|| {
            SourceError::UnexpectedShape(
                "search response has no sectionListRenderer.contents".into(),
            )
        })?;

    let mut items = Vec::new();
    let mut next_cursor: Option<Cursor> = None;
    for section in contents {
        if let Some(token) = section
            .get("continuationItemRenderer")
            .and_then(|c| c.get("continuationEndpoint"))
            .and_then(|e| e.get("continuationCommand"))
            .and_then(|c| c.get("token"))
            .and_then(|t| t.as_str())
        {
            next_cursor = Some(Cursor(token.to_string()));
            continue;
        }
        let Some(renderers) = section
            .get("itemSectionRenderer")
            .and_then(|i| i.get("contents"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for item in renderers {
            if let Some(vr) = item.get("videoRenderer") {
                if let Some(card) = video_card_from_renderer(vr, "") {
                    items.push(card);
                }
            }
            // channelRenderer / playlistRenderer / adRenderer / others:
            // skipped by design — see module docs.
        }
    }
    Ok(Page { items, next_cursor })
}

/// Parse chapters from a `/player` response:
/// `playerOverlays.playerOverlayRenderer.decoratedPlayerBarRenderer
/// .playerBarMultiMarkersMapRenderer.markersMap[].value.chapters[]`.
/// Entries without `chapters` (e.g. HEATSEEK heatmaps) are skipped.
/// Chapters are sorted by start time; a chapter without a usable title
/// or start time is skipped, never guessed.
fn parse_chapters(json: &Value) -> Vec<Chapter> {
    let Some(markers) = json
        .get("playerOverlays")
        .and_then(|o| o.get("playerOverlayRenderer"))
        .and_then(|r| r.get("decoratedPlayerBarRenderer"))
        .and_then(|r| r.get("playerBarMultiMarkersMapRenderer"))
        .and_then(|r| r.get("markersMap"))
        .and_then(|m| m.as_array())
    else {
        return vec![];
    };
    let mut chapters: Vec<Chapter> = Vec::new();
    for marker in markers {
        let Some(list) = marker
            .get("value")
            .and_then(|v| v.get("chapters"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for entry in list {
            let Some(cr) = entry.get("chapterRenderer") else {
                continue;
            };
            let title = runs_text(cr.get("title").unwrap_or(&Value::Null));
            let Some(start_ms) = cr.get("timeRangeStartMillis").and_then(|m| m.as_u64()) else {
                continue;
            };
            if title.is_empty() {
                continue;
            }
            chapters.push(Chapter {
                title,
                start_secs: start_ms as f64 / 1000.0,
            });
        }
    }
    chapters.sort_by(|a, b| {
        a.start_secs
            .partial_cmp(&b.start_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    chapters
}

/// Parse a raw `/player` response into `VideoDetails` (metadata only —
/// streams come from `parse_player_response` in `innertube_source.rs`).
pub fn parse_video_response(json: &Value, id: &VideoId) -> Result<VideoDetails> {
    let vd = json
        .get("videoDetails")
        .ok_or_else(|| SourceError::UnexpectedShape("missing videoDetails".into()))?;
    let title = vd
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let captions = json
        .get("captions")
        .and_then(|c| c.get("playerCaptionsTracklistRenderer"))
        .and_then(|r| r.get("captionTracks"))
        .and_then(|t| t.as_array())
        .map(|tracks| {
            tracks
                .iter()
                .filter_map(|t| {
                    Some(CaptionTrack {
                        language_code: t.get("languageCode")?.as_str()?.to_string(),
                        language_name: t.get("name").map(runs_text).unwrap_or_default(),
                        base_url: t.get("baseUrl")?.as_str()?.to_string(),
                        is_auto_generated: t.get("kind").and_then(|k| k.as_str()) == Some("asr"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(VideoDetails {
        id: id.clone(),
        title,
        description: vd
            .get("shortDescription")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string(),
        channel_id: ChannelId(
            vd.get("channelId")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string(),
        ),
        channel_title: vd
            .get("author")
            .and_then(|a| a.as_str())
            .unwrap_or("")
            .to_string(),
        thumbnails: thumbnails(vd),
        duration_secs: vd
            .get("lengthSeconds")
            .and_then(|l| l.as_str())
            .and_then(|s| s.parse().ok()),
        view_count: vd
            .get("viewCount")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok()),
        like_count: None,   // /player does not carry like counts; /next does.
        published_at: None, // /player does not carry publish dates; /next does.
        chapters: parse_chapters(json),
        captions,
        related: vec![], // Up Next comes from /next, not /player.
    })
}

/// Shared helper: the `tabs[].tabRenderer.content.sectionListRenderer`
/// sections of a `/browse` response (channel and playlist pages alike).
fn browse_sections(json: &Value) -> Option<&Vec<Value>> {
    json.get("contents")
        .and_then(|c| c.get("twoColumnBrowseResultsRenderer"))
        .and_then(|r| r.get("tabs"))
        .and_then(|t| t.as_array())
        .and_then(|tabs| tabs.first())
        .and_then(|t| t.get("tabRenderer"))
        .and_then(|t| t.get("content"))
        .and_then(|c| c.get("sectionListRenderer"))
        .and_then(|s| s.get("contents"))
        .and_then(|c| c.as_array())
}

/// Parse a raw `/browse` channel response into a `ChannelPage`.
/// Videos come from `gridRenderer` items in the first tab; other
/// renderers (shelves, etc.) are skipped. Subscriber counts are
/// best-effort: "1.2M subscribers" parses to None, never to a guessed 1.
pub fn parse_channel_response(json: &Value, id: &ChannelId) -> Result<ChannelPage> {
    let header = json
        .get("header")
        .and_then(|h| h.get("c4TabbedHeaderRenderer"))
        .ok_or_else(|| SourceError::UnexpectedShape("missing c4TabbedHeaderRenderer".into()))?;
    let title = header
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let description = header.get("description").map(runs_text).unwrap_or_default();
    let subscriber_count = header
        .get("subscriberCountText")
        .map(runs_text)
        .and_then(|s| parse_count(&s));
    let avatar = header.get("avatar").map(thumbnail_list).unwrap_or_default();
    let sections = browse_sections(json).ok_or_else(|| {
        SourceError::UnexpectedShape("browse response has no tab content sections".into())
    })?;
    let mut videos = Vec::new();
    for section in sections {
        let Some(contents) = section
            .get("itemSectionRenderer")
            .and_then(|i| i.get("contents"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for item in contents {
            if let Some(grid) = item.get("gridRenderer").and_then(|g| g.get("items")) {
                if let Some(arr) = grid.as_array() {
                    for entry in arr {
                        if let Some(gv) = entry.get("gridVideoRenderer") {
                            if let Some(card) = video_card_from_renderer(gv, &title) {
                                videos.push(card);
                            }
                        }
                    }
                }
            }
            // shelfRenderer and others: skipped by design.
        }
    }
    Ok(ChannelPage {
        id: id.clone(),
        title,
        description,
        thumbnails: avatar,
        subscriber_count,
        videos,
    })
}

/// Parse a raw `/browse` playlist response into a page of video cards.
/// The playlist id is not carried in the video list, so the caller keeps
/// it; the continuation token (if any) comes from
/// `playlistVideoListRenderer.continuations`.
pub fn parse_playlist_response(json: &Value) -> Result<Page<VideoCard>> {
    let sections = browse_sections(json).ok_or_else(|| {
        SourceError::UnexpectedShape("browse response has no tab content sections".into())
    })?;
    let mut items = Vec::new();
    let mut next_cursor: Option<Cursor> = None;
    for section in sections {
        let Some(contents) = section
            .get("itemSectionRenderer")
            .and_then(|i| i.get("contents"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for item in contents {
            let Some(list) = item.get("playlistVideoListRenderer") else {
                continue;
            };
            if let Some(arr) = list.get("contents").and_then(|c| c.as_array()) {
                for entry in arr {
                    if let Some(pv) = entry.get("playlistVideoRenderer") {
                        if let Some(card) = video_card_from_renderer(pv, "") {
                            items.push(card);
                        }
                    }
                }
            }
            if let Some(token) = list
                .get("continuations")
                .and_then(|c| c.as_array())
                .and_then(|c| c.first())
                .and_then(|c| c.get("nextContinuationData"))
                .and_then(|d| d.get("continuation"))
                .and_then(|t| t.as_str())
            {
                next_cursor = Some(Cursor(token.to_string()));
            }
        }
    }
    Ok(Page { items, next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn search_raw_parses_videos_skips_others() {
        let page = parse_search_response(&fixture("search_raw.json")).unwrap();
        assert_eq!(page.items.len(), 2);
        let first = &page.items[0];
        assert_eq!(first.id.0, "dQw4w9WgXcQ");
        assert_eq!(first.title, "Fixture Video One");
        assert_eq!(first.channel_title, "Fixture Channel");
        assert_eq!(first.duration_secs, Some(213));
        assert_eq!(first.view_count, Some(1_234_567));
        assert_eq!(first.published_at.as_deref(), Some("2 weeks ago"));
        assert!(!first.is_live);
        assert_eq!(first.thumbnails.len(), 2);
        // Second item is a live video ("12 watching").
        let second = &page.items[1];
        assert!(second.is_live);
        assert_eq!(second.duration_secs, Some(605));
        // Channel + playlist renderers were skipped, not counted.
        // Continuation token extracted.
        assert_eq!(
            page.next_cursor.as_ref().map(|c| c.0.as_str()),
            Some("CONT_TOKEN_abc123")
        );
    }

    #[test]
    fn duration_and_count_helpers() {
        assert_eq!(parse_duration("3:33"), Some(213));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("live"), None);
        assert_eq!(parse_count("1,234,567 views"), Some(1_234_567));
        assert_eq!(parse_count("12 watching"), Some(12));
        assert_eq!(parse_count("No views"), None);
    }

    #[test]
    fn video_from_player_fixture() {
        let d =
            parse_video_response(&fixture("player.json"), &VideoId("dQw4w9WgXcQ".into())).unwrap();
        assert_eq!(d.title, "Fixture Video");
        assert_eq!(d.channel_title, "Fixture Channel");
        assert_eq!(d.duration_secs, Some(213));
        assert_eq!(d.captions.len(), 2);
        assert!(d
            .captions
            .iter()
            .any(|c| c.language_code == "en" && !c.is_auto_generated));
        assert!(d.captions.iter().any(|c| c.is_auto_generated));
        assert_eq!(d.chapters.len(), 3);
        assert_eq!(d.chapters[0].title, "Intro");
        assert_eq!(d.chapters[0].start_secs, 0.0);
        assert_eq!(d.chapters[1].title, "Main part");
        assert_eq!(d.chapters[1].start_secs, 65.0);
        assert_eq!(d.chapters[2].title, "Outro");
        assert_eq!(d.chapters[2].start_secs, 180.0);
    }

    #[test]
    fn search_missing_shape_is_error() {
        let err = parse_search_response(&serde_json::json!({"contents": {}})).unwrap_err();
        assert!(matches!(err, SourceError::UnexpectedShape(_)));
    }

    #[test]
    fn abbreviated_counts_are_not_guessed() {
        assert_eq!(parse_count("1.2M subscribers"), None);
        assert_eq!(parse_count("3.4K views"), None);
        assert_eq!(parse_count("99 views"), Some(99));
    }

    #[test]
    fn channel_raw_parses_grid_videos() {
        let page = parse_channel_response(
            &fixture("channel_raw.json"),
            &ChannelId("UCuAXrQd1Upf2qQv8n2OQvQ".into()),
        )
        .unwrap();
        assert_eq!(page.title, "Fixture Channel");
        assert_eq!(page.description, "A fixture channel for testing.");
        // "1.2M subscribers" is abbreviated -> None, never a guessed number.
        assert_eq!(page.subscriber_count, None);
        assert_eq!(page.thumbnails.len(), 1);
        assert_eq!(page.videos.len(), 2);
        assert_eq!(page.videos[0].id.0, "dQw4w9WgXcQ");
        assert_eq!(page.videos[0].channel_title, "Fixture Channel");
        assert_eq!(page.videos[0].duration_secs, Some(213));
        assert_eq!(page.videos[1].view_count, Some(99));
    }

    #[test]
    fn playlist_raw_parses_items_and_continuation() {
        let page = parse_playlist_response(&fixture("playlist_raw.json")).unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].id.0, "dQw4w9WgXcQ");
        assert_eq!(page.items[0].duration_secs, Some(213));
        assert_eq!(page.items[1].id.0, "ccccccccccc");
        assert_eq!(
            page.next_cursor.as_ref().map(|c| c.0.as_str()),
            Some("PL_CONT_456")
        );
    }
}
