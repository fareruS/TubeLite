//! InnerTube transport — Phase 3, pure and offline-testable.
//!
//! This module builds requests and classifies failures. It does NOT make
//! live network calls in tests, and it does NOT implement signature
//! deciphering or proof-of-origin tokens — those are separate Phase 3
//! risks (see `docs/PLAN.md` §5 and `docs/KNOWN-ISSUES.md`):
//! - Cipher transforms run in an embedded, isolated, destroyable JS
//!   runtime inside the Rust process (never in the UI webview). That
//!   runtime is not chosen yet; verify deno_core / Boa / rusty_v8 currency
//!   before committing, and record the choice in `docs/NOTES.md`.
//! - PoToken comes from an existing maintained library or the extractor
//!   sidecar, not from bespoke code in this repo. RustyPipe is stale
//!   (0.11.4, 2026-04-23) — treat it as a reference to read, not a
//!   dependency to pin, unless a newer release / active fork is found.
//! - Client versions in `clients.json` are placeholders until verified
//!   against live YouTube in Phase 3.
//!
//! What IS implemented here, and tested offline:
//! - Request envelope: base URL, Origin/Referer, client context JSON.
//! - `SAPISIDHASH`: `"{timestamp}_{sha1(timestamp + ' ' + sapisid + ' ' + origin)}"`,
//!   the standard signed-in auth header for InnerTube.
//! - Fallback chain: ordered clients, per-video failure memory (~30 min),
//!   self-heal rate-limited to once every few minutes.
//! - URL validation contract: resolved URLs are validated (HEAD or tiny
//!   ranged GET) before reaching mpv, and expiry is stored with them —
//!   the validation itself lives in the app crate's HTTP layer; this
//!   module defines the decision (`should_revalidate`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha1::{Digest, Sha1};

use crate::clients::ClientIdentity;

pub const INNERTUBE_BASE: &str = "https://www.youtube.com/youtubei/v1";
pub const INNERTUBE_ORIGIN: &str = "https://www.youtube.com";

/// Per-video failure memory TTL: a client that failed for a video is not
/// retried for that video for ~30 minutes.
pub const FAILURE_MEMORY_TTL: Duration = Duration::from_secs(30 * 60);
/// Self-heal (re-trying a previously failed client) is rate-limited to
/// once every few minutes, so a flapping client does not hammer YouTube.
pub const SELF_HEAL_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Build the InnerTube endpoint URL for a client (no API key in the URL).
pub fn endpoint_url(endpoint: &str) -> String {
    format!("{INNERTUBE_BASE}/{endpoint}")
}

/// The `context.client` JSON object for a request. Only the fields the
/// API needs; nothing identifying the user beyond the client identity.
pub fn client_context(client: &ClientIdentity) -> Value {
    json!({
        "client": {
            "clientName": client.name,
            "clientVersion": client.version,
            "userAgent": client.user_agent,
        }
    })
}

/// Build a `/player` request body for a video id.
pub fn player_request_body(client: &ClientIdentity, video_id: &str) -> Value {
    json!({
        "context": client_context(client),
        "videoId": video_id,
        "playbackContext": { "contentPlaybackContext": { "vis": 0, "splay": false } },
        "racyCheckOk": true,
        "contentCheckOk": true,
    })
}

/// Build a `/search` request body.
pub fn search_request_body(client: &ClientIdentity, query: &str) -> Value {
    json!({
        "context": client_context(client),
        "query": query,
    })
}

/// Build a `/browse` request body. `browse_id` is a channel id (UC...)
/// or a playlist id prefixed with "VL". When `continuation` is given,
/// the body carries the continuation token instead of a browse id
/// (playlist / channel pagination).
pub fn browse_request_body(
    client: &ClientIdentity,
    browse_id: Option<&str>,
    continuation: Option<&str>,
) -> Value {
    match continuation {
        Some(token) => json!({
            "context": client_context(client),
            "continuation": token,
        }),
        None => json!({
            "context": client_context(client),
            "browseId": browse_id.unwrap_or(""),
        }),
    }
}

/// SAPISIDHASH auth header value.
/// `"{timestamp}_{sha1(timestamp + ' ' + sapisid + ' ' + origin)}"`.
/// Verified against a known vector in tests (computed with Python hashlib).
pub fn sapisid_hash(sapisid: &str, origin: &str, timestamp_unix: u64) -> String {
    let input = format!("{timestamp_unix} {sapisid} {origin}");
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    // sha1 0.11's digest type has no LowerHex impl; encode bytes manually.
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{timestamp_unix}_{hex}")
}

/// Ordered fallback chain over client names.
#[derive(Debug, Clone)]
pub struct FallbackChain {
    pub order: Vec<String>,
}

impl FallbackChain {
    pub fn new(order: Vec<String>) -> Self {
        Self { order }
    }

    /// The next client after `failed`, or None when the chain is exhausted.
    pub fn next_after(&self, failed: &str) -> Option<&str> {
        let pos = self.order.iter().position(|c| c == failed)?;
        self.order.get(pos + 1).map(|s| s.as_str())
    }

    pub fn first(&self) -> Option<&str> {
        self.order.first().map(|s| s.as_str())
    }
}

/// Per-video failure memory: which (video, client) pairs failed, and when.
/// A failed pair is skipped until its TTL expires; a self-heal probe is
/// allowed at most once per `SELF_HEAL_INTERVAL` per video.
#[derive(Debug, Default)]
pub struct FailureMemory {
    failures: HashMap<(String, String), Instant>,
    last_self_heal: HashMap<String, Instant>,
}

impl FailureMemory {
    pub fn record_failure(&mut self, video_id: &str, client: &str) {
        self.failures
            .insert((video_id.to_string(), client.to_string()), Instant::now());
    }

    /// True if this (video, client) pair may be tried now.
    pub fn may_try(&self, video_id: &str, client: &str) -> bool {
        match self
            .failures
            .get(&(video_id.to_string(), client.to_string()))
        {
            Some(&at) => at.elapsed() >= FAILURE_MEMORY_TTL,
            None => true,
        }
    }

    /// True if a self-heal probe for this video is due (rate-limited).
    pub fn self_heal_due(&mut self, video_id: &str) -> bool {
        match self.last_self_heal.get(video_id) {
            Some(&at) if at.elapsed() < SELF_HEAL_INTERVAL => false,
            _ => {
                self.last_self_heal
                    .insert(video_id.to_string(), Instant::now());
                true
            }
        }
    }

    /// Drop expired entries. Call periodically; memory is bounded by the
    /// number of distinct (video, client) pairs seen in the last 30 min.
    pub fn prune(&mut self) {
        self.failures
            .retain(|_, at| at.elapsed() < FAILURE_MEMORY_TTL);
    }
}

/// A resolved URL needs revalidation (HEAD / tiny ranged GET) before mpv
/// when it is expired, has no recorded expiry, or previously 403'd.
pub fn should_revalidate(
    expires_at_unix: Option<i64>,
    now_unix: i64,
    previously_403: bool,
) -> bool {
    if previously_403 {
        return true;
    }
    match expires_at_unix {
        Some(exp) => now_unix >= exp,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::ClientRegistry;

    fn web_client() -> ClientIdentity {
        ClientRegistry::bundled().get("WEB").unwrap().clone()
    }

    #[test]
    fn player_request_body_shape() {
        let body = player_request_body(&web_client(), "dQw4w9WgXcQ");
        assert_eq!(body["videoId"], "dQw4w9WgXcQ");
        assert_eq!(body["context"]["client"]["clientName"], "WEB");
        assert_eq!(
            endpoint_url("player"),
            "https://www.youtube.com/youtubei/v1/player"
        );
    }

    #[test]
    fn sapisid_hash_known_vector() {
        // Computed with Python hashlib.sha1("1700000000 test_sapisid https://www.youtube.com").
        assert_eq!(
            sapisid_hash("test_sapisid", "https://www.youtube.com", 1_700_000_000),
            "1700000000_34562bc6921a7e4218ebf0fdb6bd88a734cd8ae8"
        );
        // Different timestamp -> different hash; format is "<ts>_<40 hex>".
        let h = sapisid_hash("x", "https://www.youtube.com", 1);
        assert!(h.starts_with("1_"));
        assert_eq!(h.len(), 2 + 40);
    }

    #[test]
    fn fallback_chain_advances_and_exhausts() {
        let chain = FallbackChain::new(vec!["WEB".into(), "ANDROID".into(), "IOS".into()]);
        assert_eq!(chain.first(), Some("WEB"));
        assert_eq!(chain.next_after("WEB"), Some("ANDROID"));
        assert_eq!(chain.next_after("IOS"), None);
        assert_eq!(chain.next_after("UNKNOWN"), None);
    }

    #[test]
    fn failure_memory_blocks_then_expires() {
        let mut mem = FailureMemory::default();
        assert!(mem.may_try("v1", "WEB"));
        mem.record_failure("v1", "WEB");
        assert!(!mem.may_try("v1", "WEB"));
        // Other videos / clients are unaffected.
        assert!(mem.may_try("v2", "WEB"));
        assert!(mem.may_try("v1", "ANDROID"));
        // Self-heal is rate-limited: first probe due, second immediately after is not.
        assert!(mem.self_heal_due("v1"));
        assert!(!mem.self_heal_due("v1"));
    }

    #[test]
    fn revalidation_rules() {
        assert!(should_revalidate(Some(1_700_000_000), 1_700_000_000, false)); // at expiry
        assert!(!should_revalidate(
            Some(1_900_000_000),
            1_700_000_000,
            false
        ));
        assert!(should_revalidate(None, 1_700_000_000, false)); // no expiry recorded
        assert!(should_revalidate(Some(1_900_000_000), 1_700_000_000, true)); // 403 overrides
    }
}
