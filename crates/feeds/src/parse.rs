//! Turning a response `Value` into a [`FeedPage`]: the item walk,
//! the chips, and the continuation token.
//!
//! The walk visits the tree in document order. When it reaches a
//! video-item node (a wrapper carrying `videoRenderer` /
//! `lockupViewModel`, or a bare renderer), it attempts extraction
//! once and does not descend further — the node's children belong to
//! that item, not to the feed. An unparseable item is skipped and
//! counted in [`ParseStats`](crate::ParseStats); it is never an
//! error.
//!
//! A feed that returns zero items logs only its top-level keys at
//! warn level — never the body.

use serde_json::Value;
use tracing::warn;

use crate::continuation::continuation_token;
use crate::error::Error;
use crate::extract::{extract_item, has_item_renderer, text_of};
use crate::find::find_all;
use crate::model::{Chip, FeedPage, ParseStats};

/// Walk `root` in document order and extract every video item.
pub fn collect_items(root: &Value) -> (Vec<crate::FeedItem>, ParseStats) {
    let mut items = Vec::new();
    let mut stats = ParseStats::default();
    walk(root, &mut items, &mut stats);
    (items, stats)
}

fn walk(value: &Value, items: &mut Vec<crate::FeedItem>, stats: &mut ParseStats) {
    match value {
        Value::Object(map) => {
            if has_item_renderer(value) {
                stats.attempted += 1;
                match extract_item(value) {
                    Some(item) => items.push(item),
                    None => stats.skipped += 1,
                }
                return;
            }
            for v in map.values() {
                walk(v, items, stats);
            }
        }
        Value::Array(list) => {
            for v in list {
                walk(v, items, stats);
            }
        }
        _ => {}
    }
}

/// The filter chips in a response, in document order.
pub fn chips_of(root: &Value) -> Vec<Chip> {
    find_all(root, "chipCloudChipRenderer")
        .iter()
        .map(|node| Chip {
            text: node.get("text").and_then(text_of).unwrap_or_default(),
            token: node
                .get("navigationEndpoint")
                .and_then(|e| e.get("continuationCommand"))
                .and_then(|c| c.get("token"))
                .and_then(|t| t.as_str())
                .map(str::to_string),
            selected: node
                .get("isSelected")
                .and_then(|s| s.as_bool())
                .unwrap_or(false),
        })
        .collect()
}

fn log_zero_items(body: &Value) {
    match body {
        Value::Object(map) => {
            let keys: Vec<&str> = map.keys().map(String::as_str).collect();
            warn!(top_level_keys = ?keys, "feed returned zero items");
        }
        Value::Array(items) => {
            warn!(
                top_level_len = items.len(),
                "feed returned zero items (top-level array)"
            );
        }
        _ => {
            warn!("feed returned zero items (top-level scalar)");
        }
    }
}

/// Parse a browse/search-style response into a page: items, the
/// continuation token, and the chips. Works for home, subscriptions,
/// history, search, channel, and continuation responses — the walk
/// does not assume a `contents` key (the continuation response has
/// none; its items live under `onResponseReceivedActions`).
pub fn parse_feed(body: &Value) -> FeedPage {
    let (items, stats) = collect_items(body);
    if items.is_empty() {
        log_zero_items(body);
    }
    FeedPage {
        continuation: continuation_token(body),
        chips: chips_of(body),
        items,
        stats,
    }
}

/// Parse a `get_watch` response (a two-item JSON array
/// `[playerResponse, watchNextResponse]`) into the Up Next page.
/// Items, chips, and the continuation token are read from the
/// `secondaryResults` subtree only, so the comments continuation and
/// the end-screen items in the same response are not mistaken for
/// the Up Next list.
pub fn parse_watch_next(body: &Value) -> Result<FeedPage, Error> {
    let watch = body
        .as_array()
        .and_then(|a| a.get(1))
        .and_then(|v| v.get("watchNextResponse"))
        .ok_or_else(|| {
            Error::UnexpectedShape(
                "watch_next response must be a two-item array [playerResponse, watchNextResponse]"
                    .to_string(),
            )
        })?;
    let secondary = find_all(watch, "secondaryResults")
        .into_iter()
        .next()
        .ok_or_else(|| {
            Error::UnexpectedShape("watchNextResponse has no secondaryResults".to_string())
        })?;
    Ok(parse_feed(secondary))
}
