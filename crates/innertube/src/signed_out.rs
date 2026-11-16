//! Detecting a signed-out response.
//!
//! Ground truth: the captured signed-out home fixture
//! (`fixtures/redacted/home-replay3.response.json`, the replay that
//! came back signed out because the cookies belonged to a different
//! account) contains exactly one grid item: a `richSectionRenderer`
//! whose content is a `feedNudgeRenderer` titled "Try searching to
//! get started" ("Start watching videos to help us build a feed of
//! videos you'll love."), and zero `richItemRenderer` video items.
//! The logged-in home fixture (`fixtures/redacted/home.response.json`,
//! 43 `richItemRenderer`) contains no `feedNudgeRenderer` at all, and
//! neither do the logged-in replays.
//!
//! So the signal is the presence of a `feedNudgeRenderer` anywhere in
//! the response. It is a nudge, not an error: a signed-out browse
//! returns HTTP 200 with this body.
//!
//! Scope: browse-style responses (home, subscriptions, history).
//! A player response for an age-gated or removed video is a different
//! shape and is not covered here.

use serde_json::Value;

/// `true` when the response is a signed-out feed: it contains a
/// `feedNudgeRenderer` (the "sign in to get a feed" nudge).
pub fn is_signed_out(body: &Value) -> bool {
    contains_key(body, "feedNudgeRenderer")
}

fn contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => {
            if map.contains_key(key) {
                return true;
            }
            map.values().any(|v| contains_key(v, key))
        }
        Value::Array(items) => items.iter().any(|v| contains_key(v, key)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/redacted")
            .join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()));
        serde_json::from_str(&text).expect("parse fixture")
    }

    #[test]
    fn signed_out_home_is_detected() {
        let body = fixture("home-replay3.response.json");
        assert!(is_signed_out(&body));
    }

    #[test]
    fn logged_in_home_is_not_signed_out() {
        let body = fixture("home.response.json");
        assert!(!is_signed_out(&body));
    }

    #[test]
    fn logged_in_replay_is_not_signed_out() {
        let body = fixture("home-replay4.response.json");
        assert!(!is_signed_out(&body));
    }
}
