//! The request headers for an InnerTube call.
//!
//! Built fresh for every request (and every retry) so a refreshed
//! cookie — or a freshly computed SAPISIDHASH epoch — is always used.
//!
//! From the captured home request:
//! - `content-type` and `accept`: `application/json`
//! - `x-goog-api-format-version`: `1` (standard for InnerTube; the
//!   browser capture does not show it because the HAR only kept the
//!   headers the browser sent on that call, but it is part of the
//!   InnerTube contract)
//! - `x-youtube-client-name`: the numeric client id (`1` for WEB —
//!   verified in the capture)
//! - `x-youtube-client-version`: `2.20261112.10.00`
//! - `x-origin` and `referer`: `https://www.youtube.com`
//! - `user-agent`: the client's UA
//! - `accept-language`: the client's language, when defined
//! - `x-goog-visitor-id`: the session's visitor data, when known
//!
//! When signed in, additionally:
//! - `Cookie`: the session cookie
//! - `Authorization: SAPISIDHASH <epoch>_<sha1(...)>`
//! - `x-youtube-bootstrap-logged-in: true` — the browser sends this
//!   on signed-in calls (verified in the capture); it is omitted
//!   when signed out, matching the browser's behaviour of only
//!   asserting a login it has.
//!
//! When signed out, no `Cookie` and no `Authorization` are sent.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::auth::authorization_value;
use crate::session::Session;
use crate::{Client, INNERTUBE_ORIGIN};

/// Build the headers for one request. Pure: reads the session, writes
/// nothing. Call it again per retry — never reuse a header map across
/// retries, or a rotated cookie will be missed.
pub fn headers(client: &Client, session: &Session) -> HeaderMap {
    let mut map = HeaderMap::new();
    map.insert("content-type", HeaderValue::from_static("application/json"));
    map.insert("accept", HeaderValue::from_static("application/json"));
    map.insert(
        HeaderName::from_static("x-goog-api-format-version"),
        HeaderValue::from_static("1"),
    );
    map.insert(
        HeaderName::from_static("x-youtube-client-name"),
        HeaderValue::from_str(&client.id).expect("client id is a valid header value"),
    );
    map.insert(
        HeaderName::from_static("x-youtube-client-version"),
        HeaderValue::from_str(&client.version).expect("client version is a valid header value"),
    );
    map.insert(
        HeaderName::from_static("x-origin"),
        HeaderValue::from_static(INNERTUBE_ORIGIN),
    );
    map.insert("referer", HeaderValue::from_static(INNERTUBE_ORIGIN));
    map.insert(
        "user-agent",
        HeaderValue::from_str(&client.user_agent).expect("user agent is a valid header value"),
    );
    if let Some(lang) = &client.accept_language {
        map.insert(
            "accept-language",
            HeaderValue::from_str(lang).expect("accept language is a valid header value"),
        );
    }
    if let Some(visitor_data) = &session.visitor_data {
        map.insert(
            HeaderName::from_static("x-goog-visitor-id"),
            HeaderValue::from_str(visitor_data).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
    }
    if let Some(cookie) = &session.cookie {
        map.insert(
            "cookie",
            HeaderValue::from_str(cookie).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        if let Some(sapisid) = session.sapisid() {
            map.insert(
                "authorization",
                HeaderValue::from_str(&authorization_value(&sapisid))
                    .expect("sapisid hash is a valid header value"),
            );
        }
        map.insert(
            HeaderName::from_static("x-youtube-bootstrap-logged-in"),
            HeaderValue::from_static("true"),
        );
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Clients;

    fn web() -> Client {
        Clients::embedded().unwrap().get("WEB").unwrap().clone()
    }

    #[test]
    fn signed_out_headers_have_numeric_client_id_and_no_auth() {
        let session = Session::default();
        let map = headers(&web(), &session);
        assert_eq!(map["x-youtube-client-name"], "1");
        assert_eq!(map["x-youtube-client-version"], "2.20261112.10.00");
        assert_eq!(map["content-type"], "application/json");
        assert_eq!(map["accept"], "application/json");
        assert_eq!(map["x-goog-api-format-version"], "1");
        assert_eq!(map["x-origin"], "https://www.youtube.com");
        assert_eq!(map["referer"], "https://www.youtube.com");
        assert!(map.get("cookie").is_none());
        assert!(map.get("authorization").is_none());
        assert!(map.get("x-youtube-bootstrap-logged-in").is_none());
        assert!(map.get("x-goog-visitor-id").is_none());
    }

    #[test]
    fn signed_in_headers_carry_cookie_and_sapisidhash() {
        let session = Session {
            cookie: Some("SAPISID=test_sapisid; SID=1".to_string()),
            visitor_data: Some("CgttdzJh...".to_string()),
            ..Default::default()
        };
        let map = headers(&web(), &session);
        assert_eq!(map["cookie"], "SAPISID=test_sapisid; SID=1");
        let auth = map["authorization"].to_str().expect("auth header");
        assert!(auth.starts_with("SAPISIDHASH "));
        let value = auth.strip_prefix("SAPISIDHASH ").unwrap();
        let (epoch, hex) = value.split_once('_').unwrap();
        assert!(epoch.parse::<u64>().is_ok());
        assert_eq!(hex.len(), 40);
        assert_eq!(map["x-youtube-bootstrap-logged-in"], "true");
        assert_eq!(map["x-goog-visitor-id"], "CgttdzJh...");
    }
}
