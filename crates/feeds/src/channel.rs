//! The channel page header: name, handle, avatar, subscriber text.
//!
//! Shapes come only from the captured channel responses
//! (`fixtures/redacted/channel.response.json` and
//! `fixtures/redacted/channel2.response.json`); nothing here is
//! invented.
//!
//! ```text
//! header.pageHeaderRenderer.pageTitle                          "M2"
//! header.pageHeaderRenderer.content.pageHeaderViewModel
//!   .title.dynamicTextViewModel.text.content                   "M2"
//!   .image.decoratedAvatarViewModel.avatar.avatarViewModel
//!     .image.sources[]                        {url, width, height}
//!   .metadata.contentMetadataViewModel.metadataRows[]
//!     .metadataParts[].text.content           "@MnetM2" /
//!                                             "10.7M subscribers" /
//!                                             "33K videos"
//! metadata.channelMetadataRenderer
//!   .title / .description / .avatar.thumbnails[] / .rssUrl
//! ```
//!
//! Subscriber counts in both fixtures are abbreviated
//! (`10.7M subscribers`, `281K subscribers`), so
//! [`ChannelHeader::subscriber_count`] stays `None` — an abbreviated
//! count is never converted to a guessed number (same rule as
//! `parse_count` in `yt-source`). The raw text is kept in
//! [`ChannelHeader::subscriber_count_text`] for display.

use serde_json::Value;

use crate::extract::{text_of, thumbs_of};
use crate::model::{ChannelHeader, Thumb};

/// Parse the subscriber count out of a `"<count> subscribers"`
/// string, exactly. `"1,234 subscribers"` -> `Some(1234)`;
/// abbreviated text (`"10.7M subscribers"`, `"281K subscribers"`)
/// -> `None`, never a guessed number.
pub fn parse_subscriber_count(text: &str) -> Option<u64> {
    let number = text
        .trim()
        .strip_suffix("subscribers")
        .or_else(|| text.trim().strip_suffix("subscriber"))?
        .trim()
        .replace(',', "");
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// The largest thumbnail first (missing width counts as 0).
fn largest_first(mut thumbs: Vec<Thumb>) -> Vec<Thumb> {
    thumbs.sort_by_key(|t| std::cmp::Reverse(t.width.unwrap_or(0)));
    thumbs
}

/// Parse the channel header out of a `/browse` channel response.
/// Returns `None` when the response carries no channel header
/// (every non-channel feed) or no usable title.
pub fn parse_channel_header(body: &Value) -> Option<ChannelHeader> {
    let header = body.get("header")?.get("pageHeaderRenderer")?;
    let view_model = header.get("content")?.get("pageHeaderViewModel")?;
    let cmr = body.get("metadata")?.get("channelMetadataRenderer");

    let title = header
        .get("pageTitle")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| {
            view_model
                .get("title")
                .and_then(|t| t.get("dynamicTextViewModel"))
                .and_then(|d| d.get("text"))
                .and_then(text_of)
        })
        .or_else(|| {
            cmr.and_then(|c| c.get("title"))
                .and_then(|t| t.as_str())
                .map(str::to_string)
        })?;
    if title.is_empty() {
        return None;
    }

    // Handle and subscriber text: every `metadataParts[].text.content`
    // across all rows, in document order. Row 0 is the handle
    // (`@MnetM2`); row 1 is `10.7M subscribers` + `33K videos` —
    // but the scan does not assume row order, only the text.
    let mut handle = None;
    let mut subscriber_count_text = None;
    if let Some(rows) = view_model
        .get("metadata")
        .and_then(|m| m.get("contentMetadataViewModel"))
        .and_then(|m| m.get("metadataRows"))
        .and_then(|r| r.as_array())
    {
        for row in rows {
            let Some(parts) = row.get("metadataParts").and_then(|p| p.as_array()) else {
                continue;
            };
            for part in parts {
                let Some(text) = part.get("text").and_then(text_of) else {
                    continue;
                };
                let text = text.trim().to_string();
                if handle.is_none() && text.starts_with('@') {
                    handle = Some(text);
                } else if subscriber_count_text.is_none()
                    && text.to_lowercase().contains("subscriber")
                {
                    subscriber_count_text = Some(text);
                }
            }
        }
    }
    let subscriber_count = subscriber_count_text
        .as_deref()
        .and_then(parse_subscriber_count);

    // Avatar: the header image sources first (largest wins); the
    // `channelMetadataRenderer` avatar is the fallback.
    let mut thumbnails = view_model
        .get("image")
        .and_then(|i| i.get("decoratedAvatarViewModel"))
        .and_then(|d| d.get("avatar"))
        .and_then(|a| a.get("avatarViewModel"))
        .and_then(|a| a.get("image"))
        .and_then(|i| i.get("sources"))
        .and_then(|s| s.as_array())
        .map(|sources| {
            sources
                .iter()
                .filter_map(|s| {
                    Some(Thumb {
                        url: s.get("url")?.as_str()?.to_string(),
                        width: s.get("width").and_then(|w| w.as_u64()).map(|w| w as u32),
                        height: s.get("height").and_then(|h| h.as_u64()).map(|h| h as u32),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if thumbnails.is_empty() {
        thumbnails = cmr
            .and_then(|c| c.get("avatar"))
            .map(thumbs_of)
            .unwrap_or_default();
    }

    Some(ChannelHeader {
        title,
        handle,
        description: cmr
            .and_then(|c| c.get("description"))
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string(),
        thumbnails: largest_first(thumbnails),
        subscriber_count_text,
        subscriber_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_subscriber_counts_parse() {
        assert_eq!(parse_subscriber_count("1,234 subscribers"), Some(1234));
        assert_eq!(parse_subscriber_count("12 subscribers"), Some(12));
        assert_eq!(
            parse_subscriber_count("1,234,567 subscribers"),
            Some(1_234_567)
        );
        assert_eq!(parse_subscriber_count("1 subscriber"), Some(1));
    }

    #[test]
    fn abbreviated_subscriber_counts_are_not_guessed() {
        assert_eq!(parse_subscriber_count("10.7M subscribers"), None);
        assert_eq!(parse_subscriber_count("281K subscribers"), None);
        assert_eq!(parse_subscriber_count("1.2B subscribers"), None);
        assert_eq!(parse_subscriber_count("No subscribers"), None);
        assert_eq!(parse_subscriber_count(""), None);
    }

    #[test]
    fn no_header_is_none() {
        assert_eq!(parse_channel_header(&json!({"contents": {}})), None);
        assert_eq!(parse_channel_header(&json!({})), None);
    }

    #[test]
    fn header_without_title_is_none() {
        let body = json!({
            "header": {"pageHeaderRenderer": {"content": {"pageHeaderViewModel": {}}}},
            "metadata": {"channelMetadataRenderer": {}},
        });
        assert_eq!(parse_channel_header(&body), None);
    }
}
