//! The RSS subscription fallback: when InnerTube is unavailable,
//! fetch each channel's public feed
//! (`https://www.youtube.com/feeds/videos.xml?channel_id=<id>`)
//! and merge the entries by published date, newest first.
//!
//! The feed URL is the one the user specified; the XML shape is the
//! public YouTube RSS format (`<entry>` with `yt:videoId`, `title`,
//! `published`, `author/name`, `media:thumbnail`). Unknown elements
//! are ignored, so the parser tolerates feed variations.
//!
//! A channel whose fetch or parse fails is logged at warn level and
//! skipped; the rest still load. Only when every channel fails does
//! the call return an error.

use std::sync::Arc;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

use crate::error::Error;
use crate::model::{FeedItem, Thumb};

/// Fetch the RSS feeds for `channel_ids` concurrently (at most 8 in
/// flight), parse them, and merge by published date, newest first.
pub async fn fetch_rss(
    channel_ids: &[String],
    http: &reqwest::Client,
) -> Result<Vec<FeedItem>, Error> {
    let semaphore = Arc::new(Semaphore::new(8));
    let mut set = JoinSet::new();
    for channel_id in channel_ids {
        let semaphore = semaphore.clone();
        let http = http.clone();
        let channel_id = channel_id.clone();
        set.spawn(async move {
            let _permit = semaphore.acquire().await;
            let url = format!("https://www.youtube.com/feeds/videos.xml?channel_id={channel_id}");
            let text = http
                .get(&url)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            parse_rss(&text, &channel_id)
        });
    }
    let mut all = Vec::new();
    let mut first_error: Option<Error> = None;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Ok(items)) => all.extend(items),
            Ok(Err(e)) => {
                warn!(error = %e, "rss feed failed; channel skipped");
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
            Err(e) => {
                warn!(error = %e, "rss feed task failed; channel skipped");
                if first_error.is_none() {
                    first_error = Some(Error::UnexpectedShape(format!("rss task failed: {e}")));
                }
            }
        }
    }
    if all.is_empty() {
        if let Some(e) = first_error {
            return Err(e);
        }
    }
    Ok(merge_by_published(all))
}

/// Merge items from several feeds, newest first by published date.
/// Published dates are RFC 3339 strings, which sort correctly as
/// plain strings. Items without a date sort last.
pub fn merge_by_published(mut items: Vec<FeedItem>) -> Vec<FeedItem> {
    items.sort_by(|a, b| b.published_text.cmp(&a.published_text));
    items
}

#[derive(Default)]
struct RssEntry {
    video_id: String,
    /// The raw `<id>` text (`yt:video:<id>`), kept as a fallback for
    /// feeds without a `<yt:videoId>` element.
    id_raw: String,
    title: String,
    published: String,
    author: String,
    thumbnails: Vec<Thumb>,
}

/// Parse one channel's RSS feed XML into items. `channel_id` is the
/// id the feed was requested for (the feed itself does not repeat it
/// per entry).
pub fn parse_rss(xml: &str, channel_id: &str) -> Result<Vec<FeedItem>, Error> {
    let mut reader = Reader::from_str(xml);
    // No trim_text: it would trim the leading space of a text chunk
    // that follows an entity (`&amp;` splits "a & b" into chunks,
    // and trimming the " b" chunk loses the space). Whitespace-only
    // text between elements is ignored because `field` is unset
    // there; each completed field is trimmed at its end tag instead.
    let mut buf = Vec::new();
    let mut items = Vec::new();
    let mut entry = RssEntry::default();
    let mut in_entry = false;
    let mut in_author = false;
    let mut in_media_group = false;
    // The element the current text belongs to, as a local name.
    let mut field: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "entry" => {
                        entry = RssEntry::default();
                        in_entry = true;
                    }
                    "author" => in_author = true,
                    "group" => in_media_group = true,
                    // `<media:title>` inside `<media:group>` repeats
                    // the entry title; only the entry's own title
                    // is the item title.
                    "title" if !in_media_group => field = Some(name),
                    "published" | "videoId" | "id" => field = Some(name),
                    "name" if in_author => field = Some("authorName".to_string()),
                    _ => field = None,
                }
            }
            Ok(Event::Empty(e)) => {
                if in_entry && local_name(&e) == "thumbnail" {
                    let mut url = String::new();
                    let mut width = None;
                    let mut height = None;
                    for attr in e.attributes().flatten() {
                        let key: &str = attr.key.as_ref();
                        let value = match attr.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                        {
                            Ok(v) => v.into_owned(),
                            Err(_) => attr.value.into_owned(),
                        };
                        match key {
                            "url" => url = value,
                            "width" => width = value.parse().ok(),
                            "height" => height = value.parse().ok(),
                            _ => {}
                        }
                    }
                    if !url.is_empty() {
                        entry.thumbnails.push(Thumb { url, width, height });
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if !in_entry {
                    continue;
                }
                let raw: &str = e.as_ref();
                let text = quick_xml::escape::unescape(raw)
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| raw.to_owned());
                // Text can arrive in several events (an entity such
                // as `&amp;` splits it), so append, never assign.
                push_field(&mut entry, &field, &text);
            }
            Ok(Event::GeneralRef(e)) => {
                if !in_entry {
                    continue;
                }
                // `&amp;` and friends arrive as their own event;
                // resolve the reference and append it like text.
                let text = resolve_ref(&e);
                push_field(&mut entry, &field, &text);
            }
            Ok(Event::End(e)) => {
                let name = local_name_end(&e);
                match name.as_str() {
                    "entry" => {
                        in_entry = false;
                        if entry.video_id.is_empty() {
                            entry.video_id = entry
                                .id_raw
                                .trim()
                                .strip_prefix("yt:video:")
                                .unwrap_or(entry.id_raw.trim())
                                .to_string();
                        }
                        if !entry.video_id.is_empty() {
                            items.push(FeedItem {
                                video_id: std::mem::take(&mut entry.video_id),
                                title: std::mem::take(&mut entry.title),
                                channel_name: std::mem::take(&mut entry.author),
                                channel_id: channel_id.to_string(),
                                thumbnails: std::mem::take(&mut entry.thumbnails),
                                duration_text: String::new(),
                                published_text: std::mem::take(&mut entry.published),
                                view_count_text: String::new(),
                                badges: Vec::new(),
                            });
                        }
                        entry = RssEntry::default();
                    }
                    "author" => in_author = false,
                    "group" => in_media_group = false,
                    "title" => entry.title = entry.title.trim().to_string(),
                    "published" => entry.published = entry.published.trim().to_string(),
                    "videoId" => entry.video_id = entry.video_id.trim().to_string(),
                    "id" => entry.id_raw = entry.id_raw.trim().to_string(),
                    "name" => entry.author = entry.author.trim().to_string(),
                    _ => {}
                }
                // Whitespace after an element's text belongs to no
                // field; clear it here so it is never appended to
                // the previous field.
                field = None;
            }
            Ok(_) => {}
            Err(e) => return Err(Error::Xml(e)),
        }
        buf.clear();
    }
    Ok(items)
}

/// Append a text chunk to the field currently being read. Text can
/// arrive in several events, so callers append, never assign.
fn push_field(entry: &mut RssEntry, field: &Option<String>, text: &str) {
    match field.as_deref() {
        Some("title") => entry.title.push_str(text),
        Some("published") => entry.published.push_str(text),
        Some("videoId") => entry.video_id.push_str(text),
        Some("id") => entry.id_raw.push_str(text),
        Some("authorName") => entry.author.push_str(text),
        _ => {}
    }
}

/// Resolve a general reference (`&amp;`, `&#38;`, …) to its text.
/// Unknown named entities are kept literally rather than dropped.
fn resolve_ref(e: &quick_xml::events::BytesRef) -> String {
    match e.as_ref() {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        name => match e.resolve_char_ref() {
            Ok(Some(ch)) => ch.to_string(),
            _ => format!("&{name};"),
        },
    }
}

fn local_name(e: &quick_xml::events::BytesStart) -> String {
    e.local_name().as_ref().to_owned()
}

fn local_name_end(e: &quick_xml::events::BytesEnd) -> String {
    e.local_name().as_ref().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <id>yt:video:AAAA1111</id>
    <yt:videoId>AAAA1111</yt:videoId>
    <title>Older video &amp; friends</title>
    <published>2026-11-01T10:00:00+00:00</published>
    <author><name>Chan One</name></author>
    <media:group><media:thumbnail url="https://i.ytimg.com/vi/AAAA1111/hqdefault.jpg" width="480" height="360"/></media:group>
  </entry>
  <entry>
    <id>yt:video:BBBB2222</id>
    <yt:videoId>BBBB2222</yt:videoId>
    <title>Newer video</title>
    <published>2026-11-15T10:00:00+00:00</published>
    <author><name>Chan One</name></author>
    <media:group><media:thumbnail url="https://i.ytimg.com/vi/BBBB2222/hqdefault.jpg" width="480" height="360"/></media:group>
  </entry>
</feed>"#;

    #[test]
    fn parses_entries_and_orders_newest_first() {
        let items = parse_rss(SAMPLE, "UCchan1").expect("parse sample");
        let merged = merge_by_published(items);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].video_id, "BBBB2222");
        assert_eq!(merged[0].title, "Newer video");
        assert_eq!(merged[0].channel_name, "Chan One");
        assert_eq!(merged[0].channel_id, "UCchan1");
        assert_eq!(merged[0].published_text, "2026-11-15T10:00:00+00:00");
        assert_eq!(
            merged[0].thumbnails[0].url,
            "https://i.ytimg.com/vi/BBBB2222/hqdefault.jpg"
        );
        assert_eq!(merged[0].thumbnails[0].width, Some(480));
        assert_eq!(merged[1].video_id, "AAAA1111");
        assert_eq!(merged[1].title, "Older video & friends");
    }

    #[test]
    fn falls_back_to_id_when_videoid_is_absent() {
        let xml = SAMPLE.replace("<yt:videoId>AAAA1111</yt:videoId>", "");
        let items = parse_rss(&xml, "UCchan1").expect("parse");
        assert!(items.iter().any(|i| i.video_id == "AAAA1111"));
    }
}
