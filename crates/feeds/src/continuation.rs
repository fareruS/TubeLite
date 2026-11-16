//! Continuation tokens: both shapes that occur in the fixtures
//! (see `docs/renderer-map.md`).
//!
//! - Shape A (endpoint form, every feed): `continuationItemRenderer
//!   .continuationEndpoint.continuationCommand.token`.
//! - Shape B (button form, only watch-next Up Next, same value as A
//!   there): `continuationItemRenderer.button.buttonRenderer.command
//!   .continuationCommand.token`.
//!
//! The parser reads shape A first and falls back to shape B, and
//! skips `continuationItemRenderer` nodes that carry no token at all
//! (one occurs in the watch-next fixture).
//!
//! Chip filter tokens (`chipCloudChipRenderer.navigationEndpoint
//! .continuationCommand.token`) and chat tokens are NOT feed
//! continuations and are never returned here.

use crate::find::find_all;
use serde_json::Value;

/// The first feed continuation token found in `node` (document
/// order), or `None` when the subtree has none.
pub fn continuation_token(node: &Value) -> Option<String> {
    for renderer in find_all(node, "continuationItemRenderer") {
        if let Some(token) = renderer
            .get("continuationEndpoint")
            .and_then(|e| e.get("continuationCommand"))
            .and_then(|c| c.get("token"))
            .and_then(|t| t.as_str())
        {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
        if let Some(token) = renderer
            .get("button")
            .and_then(|b| b.get("buttonRenderer"))
            .and_then(|b| b.get("command"))
            .and_then(|c| c.get("continuationCommand"))
            .and_then(|c| c.get("token"))
            .and_then(|t| t.as_str())
        {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shape_a_wins_and_shape_b_is_the_fallback() {
        let both = json!({
            "continuationItemRenderer": {
                "continuationEndpoint": {"continuationCommand": {"token": "AAA"}},
                "button": {"buttonRenderer": {"command": {"continuationCommand": {"token": "BBB"}}}}
            }
        });
        assert_eq!(continuation_token(&both).as_deref(), Some("AAA"));
        let only_b = json!({
            "continuationItemRenderer": {
                "button": {"buttonRenderer": {"command": {"continuationCommand": {"token": "BBB"}}}}
            }
        });
        assert_eq!(continuation_token(&only_b).as_deref(), Some("BBB"));
    }

    #[test]
    fn tokenless_nodes_are_skipped() {
        let root = json!({
            "contents": [
                {"continuationItemRenderer": {}},
                {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "ZZZ"}}}}
            ]
        });
        assert_eq!(continuation_token(&root).as_deref(), Some("ZZZ"));
    }
}
