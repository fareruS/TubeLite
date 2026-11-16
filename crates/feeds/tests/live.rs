//! Live verification against youtube.com. Ignored by default:
//! the VM's egress used to hang on youtubei POSTs, and these tests
//! need the user's own cookies.
//!
//! Run with:
//!   LITUBE_COOKIES=fixtures/raw/cookies.txt \
//!     cargo test -p feeds --test live -- --ignored --nocapture
//!
//! The cookie file is read only to build the Cookie header. Cookie
//! values, SAPISID values, and hashes are never printed — only
//! counts, video ids, and titles (public YouTube data).

use std::path::PathBuf;
use std::sync::Arc;

use feeds::{parse_feed, Feeds};
use innertube::{Clients, Innertube, Session, SharedSession};
use serde_json::{json, Value};

/// Build a signed-in client from the Netscape cookie file.
/// Host-filtered to youtube.com, first occurrence wins (the dedup
/// that fixed the signed-out replays in Phase 0).
fn live_feeds() -> (Feeds, Arc<Innertube>) {
    let path = std::env::var("LITUBE_COOKIES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/cookies.txt")
        });
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read cookie file {}: {e}", path.display()));
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 7 {
            continue;
        }
        let domain = fields[0];
        if domain != "youtube.com" && !domain.ends_with(".youtube.com") {
            continue;
        }
        let name = fields[5].to_string();
        let value = fields[6].to_string();
        if !pairs.iter().any(|(n, _)| *n == name) {
            pairs.push((name, value));
        }
    }
    assert!(!pairs.is_empty(), "no youtube.com cookies in {path:?}");
    let cookie = pairs
        .iter()
        .map(|(n, v)| format!("{n}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    let session = Session {
        cookie: Some(cookie),
        ..Default::default()
    };
    assert!(session.signed_in(), "cookie file has no SAPISID");
    let inner = Innertube::new(
        Clients::embedded().expect("embedded clients"),
        SharedSession::new(session),
    )
    .expect("innertube client");
    let inner = Arc::new(inner);
    (Feeds::new(inner.clone()), inner)
}

/// Count occurrences of each key in a response, recursively.
fn inventory(root: &Value, keys: &[&str]) -> Vec<(String, usize)> {
    fn count(value: &Value, key: &str, n: &mut usize) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    if k == key {
                        *n += 1;
                    }
                    count(v, key, n);
                }
            }
            Value::Array(items) => {
                for item in items {
                    count(item, key, n);
                }
            }
            _ => {}
        }
    }
    keys.iter()
        .map(|key| {
            let mut n = 0;
            count(root, key, &mut n);
            (key.to_string(), n)
        })
        .filter(|(_, n)| *n > 0)
        .collect()
}

fn top_level_keys(body: &Value) -> Vec<String> {
    match body {
        Value::Object(map) => map.keys().cloned().collect(),
        Value::Array(items) => vec![format!("array[{}]", items.len())],
        _ => vec!["scalar".to_string()],
    }
}

#[test]
#[ignore]
fn live_home_is_signed_in_and_has_videos() {
    let (feeds, _) = live_feeds();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let page = rt.block_on(feeds.home()).expect("live home");
    println!(
        "home: {} items, {} skipped, continuation: {}, chips: {}",
        page.items.len(),
        page.stats.skipped,
        page.continuation.as_ref().map(String::len).unwrap_or(0),
        page.chips.len()
    );
    assert!(!page.items.is_empty(), "live home returned zero items");
    let first = &page.items[0];
    println!(
        "first: {} | {} | {}",
        first.video_id, first.title, first.channel_name
    );
}

#[test]
#[ignore]
fn live_subscriptions_history_search() {
    let (feeds, _) = live_feeds();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let subs = rt.block_on(feeds.subscriptions()).expect("live subs");
    println!("subscriptions: {} items", subs.items.len());
    assert!(!subs.items.is_empty());
    let history = rt.block_on(feeds.history()).expect("live history");
    println!("history: {} items", history.items.len());
    assert!(!history.items.is_empty());
    let search = rt
        .block_on(feeds.search("rust programming", None))
        .expect("live search");
    println!("search: {} items", search.items.len());
    assert!(!search.items.is_empty());
}

#[test]
#[ignore]
fn live_watch_next_and_home_continuation() {
    let (feeds, _) = live_feeds();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let home = rt.block_on(feeds.home()).expect("live home");
    assert!(!home.items.is_empty());
    let video_id = home.items[0].video_id.clone();
    let next = rt
        .block_on(feeds.watch_next(&video_id))
        .expect("live watch-next");
    println!("watch-next for {video_id}: {} items", next.items.len());
    assert!(!next.items.is_empty());
    let token = home.continuation.expect("home has a continuation token");
    let page2 = rt
        .block_on(feeds.home_continuation(&token))
        .expect("live continuation");
    println!("home continuation: {} items", page2.items.len());
    assert!(!page2.items.is_empty());
}

/// The three browses with no response fixture: fetch them raw and
/// print what renderers actually come back, so the renderer map can
/// be completed from evidence instead of guesses.
#[test]
#[ignore]
fn live_unverified_browses() {
    let (feeds, inner) = live_feeds();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let context = rt.block_on(inner.context("WEB")).expect("context");
    let keys = [
        "videoRenderer",
        "lockupViewModel",
        "playlistVideoRenderer",
        "gridRenderer",
        "richItemRenderer",
        "continuationItemRenderer",
        "shelfRenderer",
        "itemSectionRenderer",
        "playlistVideoListRenderer",
        "playlistSidebarRenderer",
        "twoColumnBrowseResultsRenderer",
    ];
    for (label, browse_id, params) in [
        ("watch-later", "VLWL", None),
        ("library-playlists", "FEplaylist_aggregation", None),
        (
            "channel-videos",
            "UCTQVIXvcHrR9jYoJ6qaBAow",
            Some("EgZ2aWRlb3PyBgQKAjoA"),
        ),
    ] {
        let mut body = json!({"context": context, "browseId": browse_id});
        if let Some(params) = params {
            body["params"] = json!(params);
        }
        let value = rt
            .block_on(inner.post("browse", "WEB", body))
            .unwrap_or_else(|e| panic!("live {label}: {e}"));
        let page = parse_feed(&value);
        println!("{label}: top-level keys: {:?}", top_level_keys(&value));
        println!("{label}: inventory: {:?}", inventory(&value, &keys));
        println!(
            "{label}: parsed {} items, {} skipped, continuation: {}",
            page.items.len(),
            page.stats.skipped,
            page.continuation.is_some()
        );
    }
    // The same three through the public functions, for the record.
    let wl = rt.block_on(feeds.watch_later()).expect("watch_later");
    println!("watch_later() via Feeds: {} items", wl.items.len());
    let lib = rt
        .block_on(feeds.library_playlists())
        .expect("library_playlists");
    println!("library_playlists() via Feeds: {} items", lib.items.len());
    let cv = rt
        .block_on(feeds.channel_videos("UCTQVIXvcHrR9jYoJ6qaBAow"))
        .expect("channel_videos");
    println!("channel_videos() via Feeds: {} items", cv.items.len());
}
