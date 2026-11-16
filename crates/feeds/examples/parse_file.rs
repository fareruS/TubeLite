//! Parse a response JSON file and print a one-line summary: item
//! count, skipped count, continuation length, chip count, and the
//! first item. Handy for checking a fresh capture (e.g. a replay
//! run on another machine) without writing a test.
//!
//! Usage: `cargo run -p feeds --example parse_file -- <response.json>`

use std::path::PathBuf;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: parse_file <response.json>");
    let text = std::fs::read_to_string(&path).expect("read response file");
    let body: serde_json::Value = serde_json::from_str(&text).expect("parse response json");
    let page = if body.is_array() {
        feeds::parse_watch_next(&body).expect("parse watch-next response")
    } else {
        feeds::parse_feed(&body)
    };
    let first = page
        .items
        .first()
        .map(|item| format!("{} | {} | {}", item.video_id, item.title, item.channel_name));
    println!(
        "{}: {} items, {} skipped, continuation: {}, chips: {}, signed_out: {}",
        PathBuf::from(&path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        page.items.len(),
        page.stats.skipped,
        page.continuation.as_ref().map(String::len).unwrap_or(0),
        page.chips.len(),
        innertube::is_signed_out(&body),
    );
    if let Some(first) = first {
        println!("  first: {first}");
    }
}
