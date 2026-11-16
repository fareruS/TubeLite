//! Client identities for the InnerTube API.
//!
//! Identities live in a bundled `clients.json` with an optional override
//! file in the app-data directory, so versions can rotate without a rebuild.
//! A client version that worked last month may be dead today — the fallback
//! chain is a feature, not a nice-to-have.
//!
//! NOTE (2026-11-08): the versions below are PLACEHOLDERS with the shape
//! documented in the Limusic analysis. Do not treat them as verified working
//! versions. Phase 3 must verify each client against live YouTube, record
//! the verified version + date in `docs/KNOWN-ISSUES.md`, and update
//! `clients.json` — never invent a working version in code.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientIdentity {
    /// e.g. "WEB", "ANDROID", "IOS", "TVHTML5"
    pub name: String,
    /// InnerTube clientId, e.g. WEB = 1, ANDROID = 3, IOS = 5, TVHTML5 = 7.
    pub client_id: u32,
    /// Client version string, e.g. "2.20250101.00.00". Rotates often.
    pub version: String,
    pub user_agent: String,
    /// Whether this client is known to require a proof-of-origin token.
    pub needs_potoken: bool,
    /// Short note: why this client is in the chain, what it is good for.
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRegistry {
    pub clients: Vec<ClientIdentity>,
    /// Ordered fallback chain for /player, by client name.
    pub player_fallback: Vec<String>,
    /// Ordered fallback chain for browse/search/next, by client name.
    pub browse_fallback: Vec<String>,
}

impl ClientRegistry {
    /// Bundled default. Override file (app-data dir) replaces this wholesale
    /// when present; it is loaded by the app crate, never by this crate.
    pub fn bundled() -> Self {
        Self {
            clients: vec![
                ClientIdentity {
                    name: "WEB".into(),
                    client_id: 1,
                    version: "2.20250101.00.00".into(),
                    user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into(),
                    needs_potoken: true,
                    note: "Primary for browse/search/next/player. May need PoToken; verify in Phase 3.".into(),
                },
                ClientIdentity {
                    name: "ANDROID".into(),
                    client_id: 3,
                    version: "19.09.37".into(),
                    user_agent: "com.google.android.youtube/19.09.37 (Linux; U; Android 14) gzip".into(),
                    needs_potoken: false,
                    note: "Fallback for /player; historically no PoToken. Verify in Phase 3.".into(),
                },
                ClientIdentity {
                    name: "IOS".into(),
                    client_id: 5,
                    version: "19.09.3".into(),
                    user_agent: "com.google.ios.youtube/19.09.3 (iPhone; iOS 17.0) gzip".into(),
                    needs_potoken: false,
                    note: "Fallback for /player. Verify in Phase 3.".into(),
                },
                ClientIdentity {
                    name: "TVHTML5".into(),
                    client_id: 7,
                    version: "7.20250101.00.00".into(),
                    user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into(),
                    needs_potoken: false,
                    note: "Fallback for /player. Verify in Phase 3.".into(),
                },
            ],
            player_fallback: vec!["WEB".into(), "ANDROID".into(), "IOS".into(), "TVHTML5".into()],
            browse_fallback: vec!["WEB".into(), "ANDROID".into()],
        }
    }

    pub fn get(&self, name: &str) -> Option<&ClientIdentity> {
        self.clients.iter().find(|c| c.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_fallback_chain_is_nonempty_and_resolvable() {
        let r = ClientRegistry::bundled();
        assert!(!r.player_fallback.is_empty());
        for name in &r.player_fallback {
            assert!(
                r.get(name).is_some(),
                "fallback client {name} missing from registry"
            );
        }
        // Serialises to the shape clients.json must have.
        let json = serde_json::to_string_pretty(&r).unwrap();
        let back: ClientRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.clients.len(), r.clients.len());
    }
}
