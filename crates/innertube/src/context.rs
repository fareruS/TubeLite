//! The `context` object for a request body.
//!
//! Minimal shape, per the spec — the browser sends far more fields
//! (remoteHost, configInfo, rolloutToken, window sizes, ...), but
//! those are per-session browser artifacts that cannot be reproduced
//! from a fixture, and the minimal context is what the API accepts:
//!
//! ```json
//! {
//!   "client": {
//!     "clientName": "WEB",
//!     "clientVersion": "2.20261112.10.00",
//!     "gl": "ID",
//!     "hl": "en",
//!     "visitorData": "..."            // only when known
//!   },
//!   "request": { "useSsl": true, "internalExperimentFlags": [] },
//!   "user": {
//!     "lockedSafetyMode": false,
//!     "onBehalfOfUser": "..."         // only when known
//!   }
//! }
//! ```
//!
//! Fields whose value is `None` are omitted entirely — never sent as
//! `null`.

use serde_json::{json, Map, Value};

use crate::session::Session;
use crate::Client;

/// Build the `context` object for a client and session.
pub fn context_for(client: &Client, session: &Session) -> Value {
    let mut client_obj = Map::new();
    client_obj.insert("clientName".to_string(), json!(client.name));
    client_obj.insert("clientVersion".to_string(), json!(client.version));
    client_obj.insert("gl".to_string(), json!(session.locale.gl));
    client_obj.insert("hl".to_string(), json!(session.locale.hl));
    if let Some(visitor_data) = &session.visitor_data {
        client_obj.insert("visitorData".to_string(), json!(visitor_data));
    }

    let mut user_obj = Map::new();
    user_obj.insert("lockedSafetyMode".to_string(), json!(false));
    if let Some(data_sync_id) = &session.data_sync_id {
        user_obj.insert("onBehalfOfUser".to_string(), json!(data_sync_id));
    }

    json!({
        "client": Value::Object(client_obj),
        "request": {
            "useSsl": true,
            "internalExperimentFlags": []
        },
        "user": Value::Object(user_obj),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Clients;

    fn web() -> Client {
        Clients::embedded().unwrap().get("WEB").unwrap().clone()
    }

    #[test]
    fn omits_none_fields() {
        let session = Session::default();
        let ctx = context_for(&web(), &session);
        assert_eq!(ctx["client"]["clientName"], "WEB");
        assert_eq!(ctx["client"]["clientVersion"], "2.20261112.10.00");
        assert_eq!(ctx["client"]["gl"], "ID");
        assert_eq!(ctx["client"]["hl"], "en");
        assert!(ctx["client"].get("visitorData").is_none());
        assert_eq!(ctx["request"]["useSsl"], true);
        assert_eq!(ctx["request"]["internalExperimentFlags"], json!([]));
        assert_eq!(ctx["user"]["lockedSafetyMode"], false);
        assert!(ctx["user"].get("onBehalfOfUser").is_none());
    }

    #[test]
    fn includes_visitor_data_and_delegated_identity_when_known() {
        let session = Session {
            visitor_data: Some("CgttdzJh...".to_string()),
            data_sync_id: Some("CAES...".to_string()),
            ..Default::default()
        };
        let ctx = context_for(&web(), &session);
        assert_eq!(ctx["client"]["visitorData"], "CgttdzJh...");
        assert_eq!(ctx["user"]["onBehalfOfUser"], "CAES...");
    }
}
