//! One test per feed against its captured fixture, with exact
//! assertions. No network: every test reads a redacted fixture from
//! `fixtures/redacted/`.

use std::path::PathBuf;

use feeds::{parse_feed, parse_watch_next};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/redacted")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
    serde_json::from_str(&text).expect("parse fixture")
}

#[test]
fn home_parses_19_videos_and_skips_5_non_videos() {
    let page = parse_feed(&fixture("home.response.json"));
    assert_eq!(page.stats.attempted, 24);
    assert_eq!(page.stats.skipped, 5);
    assert_eq!(page.items.len(), 19);
    let first = &page.items[0];
    assert_eq!(first.video_id, "qAHW8WpTNew");
    assert_eq!(first.title, "The Darkest Corners of TikTok");
    assert_eq!(first.channel_name, "Visual Venture");
    assert_eq!(first.channel_id, "UCt3JiNkefsfbA2N4SgEkoiQ");
    assert_eq!(first.view_count_text, "1M");
    assert_eq!(first.published_text, "1mo ago");
    assert_eq!(first.duration_text, "27:47");
    assert!(!first.thumbnails.is_empty());
    assert_eq!(first.thumbnails[0].width, Some(360));
    assert_eq!(first.thumbnails[0].height, Some(202));
    assert_eq!(page.continuation.as_ref().map(String::len), Some(2852));
    assert_eq!(page.chips.len(), 16);
    assert_eq!(page.chips[0].text, "All");
    assert!(page.chips[0].selected);
    assert!(page.chips[0].token.is_none());
    assert!(page.chips[1].token.is_some());
}

/// Live replays run on the user's own machine (2026-11-16): the same
/// cookies come back signed-out from this VM (the egress proxy's IP
/// is distrusted) but logged-in there. Both shapes parse, and both
/// agree on the first item.
#[test]
fn live_home_replays_from_user_machine_parse() {
    let full = parse_feed(&fixture("home-replay5.response.json"));
    assert_eq!(full.stats.attempted, 22);
    assert_eq!(full.stats.skipped, 2);
    assert_eq!(full.items.len(), 20);
    assert_eq!(full.continuation.as_ref().map(String::len), Some(2804));
    assert_eq!(full.chips.len(), 21);
    assert_eq!(full.items[0].video_id, "Y0Ej4b_vNr8");
    assert_eq!(full.items[0].channel_name, "TnM");

    let minimal = parse_feed(&fixture("home-minimal-replay3.response.json"));
    assert_eq!(minimal.stats.attempted, 24);
    assert_eq!(minimal.stats.skipped, 4);
    assert_eq!(minimal.items.len(), 20);
    assert_eq!(minimal.continuation.as_ref().map(String::len), Some(2334));
    assert_eq!(minimal.chips.len(), 19);
    assert_eq!(minimal.items[0].video_id, "Y0Ej4b_vNr8");
    assert_eq!(minimal.items[0].channel_name, "TnM");
}

#[test]
fn subscriptions_parses_112_videos_with_no_chips() {
    let page = parse_feed(&fixture("subscriptions.response.json"));
    assert_eq!(page.stats.attempted, 112);
    assert_eq!(page.stats.skipped, 0);
    assert_eq!(page.items.len(), 112);
    let first = &page.items[0];
    assert_eq!(first.video_id, "uZkQwWLqV30");
    assert!(first.title.contains("Xiaomi"));
    assert_eq!(first.channel_name, "GadgetIn");
    assert_eq!(first.view_count_text, "329K");
    assert_eq!(first.published_text, "4h ago");
    assert_eq!(page.continuation.as_ref().map(String::len), Some(204));
    assert!(page.chips.is_empty());
}

#[test]
fn history_parses_196_videos_without_age_parts() {
    let page = parse_feed(&fixture("history.response.json"));
    assert_eq!(page.stats.attempted, 196);
    assert_eq!(page.stats.skipped, 0);
    assert_eq!(page.items.len(), 196);
    let first = &page.items[0];
    assert_eq!(first.video_id, "a03aBrwGqx8");
    assert_eq!(first.title, "The Mushrooms 🍄 Weren't Edible 😳");
    assert_eq!(first.channel_name, "Chubbyemu");
    assert_eq!(first.view_count_text, "1.3M");
    // History parts carry no age: [channel, views].
    assert_eq!(first.published_text, "");
    assert_eq!(page.continuation.as_ref().map(String::len), Some(120));
    assert!(page.chips.is_empty());
}

#[test]
fn search_parses_18_classic_renderers_and_skips_4_lockups() {
    let page = parse_feed(&fixture("search.response.json"));
    assert_eq!(page.stats.attempted, 22);
    assert_eq!(page.stats.skipped, 4);
    assert_eq!(page.items.len(), 18);
    let first = &page.items[0];
    assert_eq!(first.video_id, "7wuaLanSqDs");
    assert_eq!(first.title, "Stages of Learning Rust");
    assert_eq!(first.channel_name, "bigboxSWE");
    assert_eq!(first.channel_id, "UC5--wS0Ljbin1TjWQX6eafA");
    assert_eq!(first.view_count_text, "167,488 views");
    assert_eq!(first.published_text, "1y ago");
    assert_eq!(first.duration_text, "3:47");
    assert_eq!(first.badges.len(), 1);
    assert_eq!(first.badges[0].label, "Verified");
    assert_eq!(
        first.badges[0].style.as_deref(),
        Some("BADGE_STYLE_TYPE_VERIFIED")
    );
    assert_eq!(first.thumbnails[0].width, Some(360));
    assert_eq!(page.continuation.as_ref().map(String::len), Some(814));
    assert_eq!(page.chips.len(), 7);
}

#[test]
fn channel_home_tab_parses_its_shelf_videos() {
    let page = parse_feed(&fixture("channel.response.json"));
    assert_eq!(page.items.len(), 102);
    let first = &page.items[0];
    assert_eq!(first.video_id, "mkJRSPYjX8k");
    assert_eq!(first.channel_name, "M2");
    // This shelf item names only its channel: no views, no age.
    assert_eq!(first.view_count_text, "");
    assert_eq!(first.published_text, "");
}

#[test]
fn channel_header_comes_from_page_header_not_first_item() {
    let page = parse_feed(&fixture("channel.response.json"));
    let header = page.channel.as_ref().expect("channel header");
    assert_eq!(header.title, "M2");
    assert_eq!(header.handle.as_deref(), Some("@MnetM2"));
    // Largest header avatar source (s160); the
    // channelMetadataRenderer avatar (s900) is only the fallback.
    assert_eq!(
        header.thumbnails.first().map(|t| t.url.as_str()),
        Some("https://yt3.googleusercontent.com/-CMwNSrdUuj-abbE1fB1ml62omQHViexCOp4IrDmans8QgqKC153dCgPk9wtBLJfXEP1gNGa=s160-c-k-c0x00ffffff-no-rj")
    );
    assert_eq!(header.thumbnails.first().and_then(|t| t.width), Some(160));
    // "10.7M subscribers" is abbreviated: kept as text, never
    // converted to a guessed number.
    assert_eq!(
        header.subscriber_count_text.as_deref(),
        Some("10.7M subscribers")
    );
    assert_eq!(header.subscriber_count, None);
    assert!(header.description.starts_with("Mnet Digital Studio, M2"));
    // The videos list is unchanged by the header parse.
    assert_eq!(page.items.len(), 102);
}

#[test]
fn channel2_header_parses_too() {
    let page = parse_feed(&fixture("channel2.response.json"));
    let header = page.channel.as_ref().expect("channel header");
    assert_eq!(header.title, "bigboxSWE");
    assert_eq!(header.handle.as_deref(), Some("@bigboxSWE"));
    assert_eq!(
        header.thumbnails.first().map(|t| t.url.as_str()),
        Some("https://yt3.googleusercontent.com/z1YVihkpWgDP7h8bCNZb5qJ0N3Eu26VZmwyjbdpaNmkGfgPW29JIMQdpaP2PEY3vjhARWmG2yA=s160-c-k-c0x00ffffff-no-rj")
    );
    assert_eq!(
        header.subscriber_count_text.as_deref(),
        Some("281K subscribers")
    );
    assert_eq!(header.subscriber_count, None);
    assert!(header.description.starts_with("bigboxSWE"));
    assert_eq!(page.items.len(), 13);
}

#[test]
fn non_channel_feeds_have_no_channel_header() {
    let home = parse_feed(&fixture("home.response.json"));
    assert!(home.channel.is_none());
    let search = parse_feed(&fixture("search.response.json"));
    assert!(search.channel.is_none());
}

#[test]
fn channel2_shelf_item_without_a_channel_part() {
    let page = parse_feed(&fixture("channel2.response.json"));
    assert_eq!(page.items.len(), 13);
    let first = &page.items[0];
    assert_eq!(first.video_id, "PE8nJxZq2qI");
    // Its parts are [views, age] with no channel link, so the
    // channel is honestly empty rather than guessed.
    assert_eq!(first.channel_name, "");
    assert_eq!(first.view_count_text, "293K views");
    assert_eq!(first.published_text, "3 days ago");
}

#[test]
fn watch_next_parses_the_up_next_list_only() {
    let page = parse_watch_next(&fixture("watch-next.response.json")).expect("watch-next");
    assert_eq!(page.items.len(), 20);
    let first = &page.items[0];
    assert_eq!(first.video_id, "DMBc0F5fw9Y");
    assert_eq!(first.channel_name, "Tellwyn");
    // Up Next parts here are [channel, "and 3 more"]: no views, no age.
    assert_eq!(first.view_count_text, "");
    // The Up Next token (1768), not the comments token (78).
    assert_eq!(page.continuation.as_ref().map(String::len), Some(1768));
    assert_eq!(page.chips.len(), 5);
}

#[test]
fn continuation_response_parses_its_20_items_and_next_token() {
    let page = parse_feed(&fixture("continuation.response.json"));
    assert_eq!(page.stats.attempted, 24);
    assert_eq!(page.stats.skipped, 4);
    assert_eq!(page.items.len(), 20);
    let first = &page.items[0];
    assert_eq!(first.video_id, "r55_nONqeCM");
    assert_eq!(first.title, "Why you should always put your phone away");
    assert_eq!(first.channel_name, "MrBallen");
    assert_eq!(first.view_count_text, "1.6M");
    assert_eq!(first.published_text, "11d ago");
    assert_eq!(page.continuation.as_ref().map(String::len), Some(4028));
}

#[test]
fn signed_out_home_parses_zero_items() {
    let body = fixture("home-replay3.response.json");
    assert!(innertube::is_signed_out(&body));
    let page = parse_feed(&body);
    assert!(page.items.is_empty());
    assert!(page.continuation.is_none());
}

#[test]
fn removing_one_renderer_removes_only_that_item() {
    let mut body = fixture("home.response.json");
    let before = parse_feed(&body);
    assert_eq!(before.items.len(), 19);
    let removed_id = before.items[0].video_id.clone();
    // Delete the first lockupViewModel node, in place.
    fn remove_first_lockup(value: &mut Value) -> bool {
        match value {
            Value::Object(map) => {
                if map.remove("lockupViewModel").is_some() {
                    return true;
                }
                map.values_mut().any(remove_first_lockup)
            }
            Value::Array(items) => items.iter_mut().any(remove_first_lockup),
            _ => false,
        }
    }
    assert!(remove_first_lockup(&mut body));
    let after = parse_feed(&body);
    assert_eq!(after.stats.attempted, before.stats.attempted - 1);
    assert_eq!(after.stats.skipped, before.stats.skipped);
    assert_eq!(after.items.len(), before.items.len() - 1);
    assert!(!after.items.iter().any(|i| i.video_id == removed_id));
    let mut before_ids: Vec<&str> = before
        .items
        .iter()
        .map(|i| i.video_id.as_str())
        .filter(|id| *id != removed_id)
        .collect();
    let mut after_ids: Vec<&str> = after.items.iter().map(|i| i.video_id.as_str()).collect();
    before_ids.sort_unstable();
    after_ids.sort_unstable();
    assert_eq!(before_ids, after_ids);
}

#[test]
fn rss_sample_parses_and_orders_newest_first() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rss_sample.xml");
    let xml = std::fs::read_to_string(&path).expect("read rss sample");
    let items = feeds::parse_rss(&xml, "UCchan1").expect("parse rss");
    let merged = feeds::merge_by_published(items);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].video_id, "BBBB2222CCCC");
    assert_eq!(merged[0].title, "Newer video");
    assert_eq!(merged[0].channel_name, "Chan One");
    assert_eq!(merged[0].channel_id, "UCchan1");
    assert_eq!(merged[0].published_text, "2026-11-15T10:00:00+00:00");
    assert_eq!(
        merged[0].thumbnails[0].url,
        "https://i.ytimg.com/vi/BBBB2222CCCC/hqdefault.jpg"
    );
    assert_eq!(merged[1].video_id, "AAAA1111BBBB");
    assert_eq!(merged[1].title, "Older video & friends");
}
