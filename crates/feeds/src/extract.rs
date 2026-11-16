//! Item extractors: one per renderer generation found in the
//! fixtures, plus a dispatcher that tries each generation per node.
//!
//! Generations (see `docs/renderer-map.md`):
//! - Gen 1 — `videoRenderer` (classic): search only.
//! - Gen 2 — `lockupViewModel` (new): home, subscriptions, history,
//!   watch-next, channel shelves.
//!
//! An extractor returns `None` when the node is not its generation
//! or when a required field (video id, title) is missing. `None` is
//! never an error — the caller skips the item and counts it (see
//! [`ParseStats`](crate::ParseStats)).

use crate::model::{Badge, FeedItem, Thumb};

/// Read a text field: a plain string, a `simpleText`, or `runs`
/// joined. Returns `None` when the value carries no text.
pub fn text_of(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(s)) = map.get("simpleText") {
                return Some(s.clone());
            }
            if let Some(serde_json::Value::Array(runs)) = map.get("runs") {
                let mut out = String::new();
                for run in runs {
                    if let Some(serde_json::Value::String(text)) = run.get("text") {
                        out.push_str(text);
                    }
                }
                if !out.is_empty() {
                    return Some(out);
                }
            }
            if let Some(serde_json::Value::String(s)) = map.get("content") {
                return Some(s.clone());
            }
            None
        }
        _ => None,
    }
}

/// Read a `thumbnails[]` list (`url`, `width`, `height`).
pub fn thumbs_of(value: &serde_json::Value) -> Vec<Thumb> {
    let Some(serde_json::Value::Array(list)) = value.get("thumbnails") else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|t| {
            let url = t.get("url")?.as_str()?.to_string();
            Some(Thumb {
                url,
                width: t.get("width").and_then(|w| w.as_u64()).map(|w| w as u32),
                height: t.get("height").and_then(|h| h.as_u64()).map(|h| h as u32),
            })
        })
        .collect()
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.is_empty())
}

/// The browse id of a channel link: `runs[].navigationEndpoint
/// .browseEndpoint.browseId`, first one found.
fn channel_id_of(runs_value: &serde_json::Value) -> Option<String> {
    let serde_json::Value::Array(runs) = runs_value.get("runs")? else {
        return None;
    };
    runs.iter().find_map(|run| {
        run.get("navigationEndpoint")?
            .get("browseEndpoint")?
            .get("browseId")?
            .as_str()
            .map(str::to_string)
    })
}

/// Gen 1 — the classic `videoRenderer` (search). The node is the
/// `videoRenderer` object itself.
pub fn extract_video_renderer(node: &serde_json::Value) -> Option<FeedItem> {
    let video_id = non_empty(node.get("videoId")?.as_str().map(str::to_string))?;
    let title = non_empty(text_of(node.get("title")?))?;
    let owner = node.get("ownerText");
    let long_byline = node.get("longBylineText");
    let short_byline = node.get("shortBylineText");
    let channel_name = owner
        .and_then(text_of)
        .or_else(|| long_byline.and_then(text_of))
        .or_else(|| short_byline.and_then(text_of))
        .unwrap_or_default();
    let channel_id = long_byline
        .and_then(channel_id_of)
        .or_else(|| owner.and_then(channel_id_of))
        .unwrap_or_default();
    let duration_text = node
        .get("lengthText")
        .and_then(text_of)
        .or_else(|| {
            // Fallback: the time overlay on the thumbnail.
            let serde_json::Value::Array(overlays) = node.get("thumbnailOverlays")? else {
                return None;
            };
            overlays
                .iter()
                .find_map(|o| text_of(o.get("thumbnailOverlayTimeStatusRenderer")?.get("text")?))
        })
        .unwrap_or_default();
    let badges = node
        .get("ownerBadges")
        .and_then(|b| b.as_array())
        .map(|badges| {
            badges
                .iter()
                .filter_map(|b| {
                    let renderer = b.get("metadataBadgeRenderer")?;
                    let label = renderer
                        .get("label")
                        .and_then(|l| l.as_str())
                        .map(str::to_string)
                        .or_else(|| {
                            renderer
                                .get("tooltip")
                                .and_then(|t| t.as_str())
                                .map(str::to_string)
                        })?;
                    if label.is_empty() {
                        return None;
                    }
                    Some(Badge {
                        label,
                        style: renderer
                            .get("style")
                            .and_then(|s| s.as_str())
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(FeedItem {
        video_id,
        title,
        channel_name,
        channel_id,
        thumbnails: node.get("thumbnail").map(thumbs_of).unwrap_or_default(),
        duration_text,
        published_text: node
            .get("publishedTimeText")
            .and_then(text_of)
            .unwrap_or_default(),
        view_count_text: node
            .get("viewCountText")
            .and_then(text_of)
            .unwrap_or_default(),
        badges,
    })
}

/// One metadata part of a `lockupViewModel`: its text and, when the
/// part is a link, the channel browse id it points to.
struct LockupPart {
    text: String,
    channel_id: Option<String>,
}

fn lockup_parts(node: &serde_json::Value) -> Vec<LockupPart> {
    let Some(parts) = node
        .get("metadata")
        .and_then(|m| m.get("lockupMetadataViewModel"))
        .and_then(|m| m.get("metadata"))
        .and_then(|m| m.get("contentMetadataViewModel"))
        .and_then(|m| m.get("metadataRows"))
        .and_then(|r| r.as_array())
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("metadataParts"))
        .and_then(|p| p.as_array())
    else {
        return Vec::new();
    };
    parts
        .iter()
        .map(|part| {
            let text = part.get("text").and_then(text_of).unwrap_or_default();
            let channel_id = part.get("text").and_then(|t| {
                let serde_json::Value::Array(runs) = t.get("commandRuns")? else {
                    return None;
                };
                runs.iter().find_map(|run| {
                    run.get("onTap")?
                        .get("innertubeCommand")?
                        .get("browseEndpoint")?
                        .get("browseId")?
                        .as_str()
                        .map(str::to_string)
                })
            });
            LockupPart { text, channel_id }
        })
        .collect()
}

/// Whether a metadata part looks like a view count: `1M`, `513K`,
/// `1.3M`, `167,488 views`, `No views`. A channel name that merely
/// starts with a digit (`3Blue1Brown`) does not match — the whole
/// part must be a number with an optional K/M/B suffix and an
/// optional "views" suffix.
fn is_view_count(s: &str) -> bool {
    if s == "No views" {
        return true;
    }
    let s = s
        .strip_suffix(" views")
        .or_else(|| s.strip_suffix(" view"))
        .unwrap_or(s);
    let num = match s.strip_suffix(['K', 'M', 'B']) {
        Some(n) => n,
        None => s,
    };
    !num.is_empty()
        && num.chars().any(|c| c.is_ascii_digit())
        && num
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
}

/// Gen 2 — the new `lockupViewModel` (home, subscriptions, history,
/// watch-next, channel shelves). The node is the `lockupViewModel`
/// object itself. Only `LOCKUP_CONTENT_TYPE_VIDEO` nodes are items;
/// playlists, channels, and ads (`feedAdMetadataViewModel`) return
/// `None`.
pub fn extract_lockup(node: &serde_json::Value) -> Option<FeedItem> {
    if node.get("contentType")?.as_str()? != "LOCKUP_CONTENT_TYPE_VIDEO" {
        return None;
    }
    let video_id = non_empty(node.get("contentId")?.as_str().map(str::to_string))?;
    let title = non_empty(
        node.get("metadata")?
            .get("lockupMetadataViewModel")?
            .get("title")
            .and_then(text_of),
    )?;
    let parts = lockup_parts(node);
    // The channel is the part that links to a channel; when no part
    // links anywhere (history, watch-next), it is the first part —
    // unless that part is itself a view count or an age, in which
    // case the node names no channel at all (a channel shelf item).
    let channel_index = parts
        .iter()
        .position(|p| p.channel_id.is_some())
        .or_else(|| {
            parts.first().and_then(|p| {
                if !is_view_count(&p.text) && !p.text.ends_with(" ago") {
                    Some(0)
                } else {
                    None
                }
            })
        });
    let (channel_name, channel_id) = channel_index
        .map(|i| {
            (
                parts[i].text.clone(),
                parts[i].channel_id.clone().unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    // Age is the last part ending in " ago"; views is the first
    // remaining part that looks like a count.
    let age_index = parts.iter().rposition(|p| p.text.ends_with(" ago"));
    let published_text = age_index.map(|i| parts[i].text.clone()).unwrap_or_default();
    let view_count_text = parts
        .iter()
        .enumerate()
        .find(|(i, p)| Some(*i) != channel_index && Some(*i) != age_index && is_view_count(&p.text))
        .map(|(_, p)| p.text.clone())
        .unwrap_or_default();
    let thumbnails = node
        .get("contentImage")
        .and_then(|i| i.get("thumbnailViewModel"))
        .and_then(|t| t.get("image"))
        .map(|image| {
            let Some(serde_json::Value::Array(sources)) = image.get("sources") else {
                return Vec::new();
            };
            sources
                .iter()
                .filter_map(|s| {
                    let url = s.get("url")?.as_str()?.to_string();
                    Some(Thumb {
                        url,
                        width: s.get("width").and_then(|w| w.as_u64()).map(|w| w as u32),
                        height: s.get("height").and_then(|h| h.as_u64()).map(|h| h as u32),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let duration_text = node
        .get("contentImage")
        .and_then(|i| i.get("thumbnailViewModel"))
        .and_then(|t| t.get("overlays"))
        .and_then(|o| o.as_array())
        .and_then(|overlays| {
            overlays.iter().find_map(|o| {
                let serde_json::Value::Array(badges) =
                    o.get("thumbnailBottomOverlayViewModel")?.get("badges")?
                else {
                    return None;
                };
                badges.iter().find_map(|b| {
                    b.get("thumbnailBadgeViewModel")?
                        .get("text")?
                        .as_str()
                        .map(str::to_string)
                })
            })
        })
        .unwrap_or_default();
    Some(FeedItem {
        video_id,
        title,
        channel_name,
        channel_id,
        thumbnails,
        duration_text,
        published_text,
        view_count_text,
        badges: Vec::new(),
    })
}

/// The dispatcher: tries each generation per node. `node` may be a
/// wrapper (e.g. `richItemRenderer.content`) or a bare renderer.
/// Video first, then lockup — a node never carries both, but the
/// order is fixed so the result is deterministic.
pub fn extract_item(node: &serde_json::Value) -> Option<FeedItem> {
    if let Some(renderer) = node.get("videoRenderer") {
        if let Some(item) = extract_video_renderer(renderer) {
            return Some(item);
        }
    }
    if let Some(lockup) = node.get("lockupViewModel") {
        if let Some(item) = extract_lockup(lockup) {
            return Some(item);
        }
    }
    // Bare renderer nodes (already unwrapped by the walk). The bare
    // videoRenderer check is strict on purpose: a bare node needs a
    // thumbnail and a byline, so player nodes that merely carry a
    // videoId and a title (channel trailer, end-screen items) are not
    // mistaken for feed videos.
    if node.get("videoId").is_some()
        && node.get("title").is_some()
        && node.get("thumbnail").is_some()
        && (node.get("ownerText").is_some()
            || node.get("longBylineText").is_some()
            || node.get("shortBylineText").is_some())
    {
        if let Some(item) = extract_video_renderer(node) {
            return Some(item);
        }
    }
    if node.get("contentId").is_some() && node.get("contentType").is_some() {
        if let Some(item) = extract_lockup(node) {
            return Some(item);
        }
    }
    None
}

/// Whether `node` is a video-item node (wrapper or bare renderer) —
/// used by the walk to decide what counts as an attempted item. The
/// bare checks are strict (see [`extract_item`]) so player nodes
/// that merely carry a video id and a title are not counted.
pub fn has_item_renderer(node: &serde_json::Value) -> bool {
    node.get("videoRenderer").is_some()
        || node.get("lockupViewModel").is_some()
        || (node.get("videoId").is_some()
            && node.get("title").is_some()
            && node.get("thumbnail").is_some()
            && (node.get("ownerText").is_some()
                || node.get("longBylineText").is_some()
                || node.get("shortBylineText").is_some()))
        || (node.get("contentId").is_some() && node.get("contentType").is_some())
}
