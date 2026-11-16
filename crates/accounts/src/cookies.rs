//! The harvested session: one `Cookie` header value plus the optional
//! identity fields the InnerTube context carries.
//!
//! A `CookieSet` is built from exactly one of three sources:
//!
//! - the webview's cookie store (the login popup / hidden refresh),
//!   via [`CookieSet::from_pairs`] (no validation — the caller checks
//!   [`CookieSet::signed_in`]);
//! - a raw `Cookie` header string (cookie import), via
//!   [`parse_header`];
//! - a Netscape cookie export (cookie import), via
//!   [`parse_netscape`].
//!
//! The import paths keep only youtube.com and google.com cookies and
//! require a SAPISID (or `__Secure-3PAPISID`); anything else is an
//! error, not a silent empty session.
//!
//! The cookie value itself is a [`SecretString`]: it never prints.

use innertube::{sapisid_value, Session};

use crate::{Error, SecretString};

/// The cookies for one signed-in session, as the `Cookie` header the
/// InnerTube client sends.
#[derive(Clone)]
pub struct CookieSet {
    /// `name=value; name=value` — the full `Cookie` header.
    pub cookie: SecretString,
    /// `visitorData` for `context.client`, when known (popup harvests
    /// do not know it yet; the app learns it from a browse response).
    pub visitor_data: Option<String>,
    /// Delegated identity for `context.user.onBehalfOfUser`, when known.
    pub data_sync_id: Option<String>,
}

impl std::fmt::Debug for CookieSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CookieSet")
            .field("cookie", &self.cookie)
            .field("signed_in", &self.signed_in())
            .field("visitor_data", &self.visitor_data.is_some())
            .field("data_sync_id", &self.data_sync_id.is_some())
            .finish()
    }
}

impl CookieSet {
    /// Build from `name, value` pairs. Empty values are dropped (an
    /// empty value is a deletion, not a cookie). Pairs are kept in the
    /// order given. No validation: the caller checks
    /// [`CookieSet::signed_in`] (the popup harvest loop does).
    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let cookie = pairs
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        Self {
            cookie: SecretString::new(cookie),
            visitor_data: None,
            data_sync_id: None,
        }
    }

    /// Signed in means the set carries a SAPISID (or its
    /// `__Secure-3PAPISID` fallback) — the value SAPISIDHASH is
    /// computed from.
    pub fn signed_in(&self) -> bool {
        self.sapisid().is_some()
    }

    /// The SAPISID value for SAPISIDHASH, if the set has one.
    pub fn sapisid(&self) -> Option<String> {
        sapisid_value(self.cookie.expose())
    }

    /// The account id for this session: `ga-` + hex(md5(SAPISID)).
    /// Stable for the same SAPISID, and it never contains the
    /// SAPISID itself — the id is safe to show in the UI and logs.
    /// `None` when the set has no SAPISID.
    pub fn account_id(&self) -> Option<String> {
        let sapisid = self.sapisid()?;
        Some(format!("ga-{:x}", md5::compute(sapisid.as_bytes())))
    }

    /// The [`Session`] the InnerTube client runs on.
    pub fn to_session(&self) -> Session {
        Session {
            cookie: Some(self.cookie.expose().to_string()),
            visitor_data: self.visitor_data.clone(),
            data_sync_id: self.data_sync_id.clone(),
            locale: Default::default(),
        }
    }
}

/// Whether a cookie domain is one we keep: youtube.com or google.com
/// (bare host or any subdomain).
fn kept_domain(domain: &str) -> bool {
    let domain = domain.strip_prefix('.').unwrap_or(domain);
    domain == "youtube.com"
        || domain.ends_with(".youtube.com")
        || domain == "google.com"
        || domain.ends_with(".google.com")
}

/// Parse a raw `Cookie` header string (`"a=b; c=d"`), as copied from
/// a youtubei request in DevTools or pasted by the user.
///
/// A raw header carries no domain per cookie, so there is nothing to
/// filter by: every pair in it was sent to youtube.com, so every pair
/// is a youtube.com cookie by construction. Malformed segments are
/// dropped rather than failing the whole set. Requires a SAPISID (or
/// `__Secure-3PAPISID`), else [`Error::NotSignedIn`].
pub fn parse_header(text: &str) -> Result<CookieSet, Error> {
    let pairs = text.split(';').filter_map(|segment| {
        let (name, value) = segment.split_once('=')?;
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            return None;
        }
        Some((name.to_string(), value.to_string()))
    });
    let set = CookieSet::from_pairs(pairs);
    if set.cookie.is_empty() {
        return Err(Error::NoCookies);
    }
    if !set.signed_in() {
        return Err(Error::NotSignedIn);
    }
    Ok(set)
}

/// Parse a Netscape cookie export (the format Chrome's cookie export
/// and the Phase 0 replay pack use) into a `CookieSet`.
///
/// Rules, shared with the replay tool (`crates/tools/replay`): lines
/// starting with `#` and blank lines are skipped; a line has 7
/// tab-separated fields (`domain, flag, path, secure, expires, name,
/// value`); only youtube.com and google.com cookies are kept; the
/// first occurrence of a name wins (an export can hold several
/// conflicting copies of one cookie, and the later ones are stale).
/// Requires a SAPISID (or `__Secure-3PAPISID`), else
/// [`Error::NotSignedIn`]; no kept cookies at all is
/// [`Error::NoCookies`].
pub fn parse_netscape(text: &str) -> Result<CookieSet, Error> {
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
        if !kept_domain(fields[0]) {
            continue;
        }
        let name = fields[5].to_string();
        let value = fields[6].to_string();
        if !pairs.iter().any(|(n, _)| *n == name) {
            pairs.push((name, value));
        }
    }
    if pairs.is_empty() {
        return Err(Error::NoCookies);
    }
    let set = CookieSet::from_pairs(pairs);
    if !set.signed_in() {
        return Err(Error::NotSignedIn);
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_parses_and_prefers_sapisid() {
        let set = parse_header(
            "VISITOR_INFO1_LIVE=abc; __Secure-3PAPISID=fallback; SAPISID=real; empty=; =noname",
        )
        .unwrap();
        assert!(set.signed_in());
        assert_eq!(set.sapisid().as_deref(), Some("real"));
        assert!(!set.cookie.expose().contains("empty="));
        assert!(!set.cookie.expose().contains("=noname"));
    }

    #[test]
    fn header_without_sapisid_errors() {
        assert!(matches!(
            parse_header("VISITOR_INFO1_LIVE=abc; PREF=hl=en"),
            Err(Error::NotSignedIn)
        ));
        assert!(matches!(parse_header(""), Err(Error::NoCookies)));
        assert!(matches!(parse_header("   "), Err(Error::NoCookies)));
    }

    #[test]
    fn netscape_keeps_youtube_and_google_and_dedupes_first_wins() {
        let text = "# Netscape HTTP Cookie File\n\
            .example.com\tTRUE\t/\tTRUE\t0\tSID\tother\n\
            .google.com\tTRUE\t/\tTRUE\t0\tSID\tgoog\n\
            .youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tfirst\n\
            youtube.com\tTRUE\t/\tFALSE\t0\tVISITOR_INFO1_LIVE\tv1\n\
            .youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tsecond\n\
            .youtube.com\tTRUE\t/\tTRUE\t0\t__Secure-3PAPISID\tfb\n";
        let set = parse_netscape(text).unwrap();
        let cookie = set.cookie.expose();
        assert_eq!(set.sapisid().as_deref(), Some("first"));
        assert!(cookie.contains("VISITOR_INFO1_LIVE=v1"));
        assert!(cookie.contains("SID=goog"));
        assert!(!cookie.contains("SID=other"));
    }

    #[test]
    fn netscape_with_no_kept_cookies_errors() {
        assert!(matches!(
            parse_netscape(".example.com\tTRUE\t/\tTRUE\t0\tSID\tx\n"),
            Err(Error::NoCookies)
        ));
    }

    #[test]
    fn netscape_without_sapisid_errors() {
        assert!(matches!(
            parse_netscape(".youtube.com\tTRUE\t/\tTRUE\t0\tVISITOR_INFO1_LIVE\tv1\n"),
            Err(Error::NotSignedIn)
        ));
    }

    #[test]
    fn account_id_is_stable_and_hides_the_sapisid() {
        // md5("abc") = 900150983cd24fb0d6963f7d28e17f72.
        let set = parse_header("SAPISID=abc; VISITOR_INFO1_LIVE=v1").unwrap();
        let id = set.account_id().unwrap();
        assert_eq!(id, "ga-900150983cd24fb0d6963f7d28e17f72");
        assert!(!id.contains("abc"));
        // Same SAPISID, different other cookies → same id.
        let other = parse_header("SAPISID=abc; PREF=hl=id").unwrap();
        assert_eq!(other.account_id().as_deref(), Some(id.as_str()));
        // The 3PAPISID fallback derives the id from that value.
        let fallback = parse_header("__Secure-3PAPISID=abc; VISITOR_INFO1_LIVE=v1").unwrap();
        assert_eq!(fallback.account_id().as_deref(), Some(id.as_str()));
        // No SAPISID → no id.
        let unsigned = CookieSet::from_pairs([("VISITOR_INFO1_LIVE".into(), "v1".into())]);
        assert_eq!(unsigned.account_id(), None);
    }

    #[test]
    fn cookie_set_debug_never_prints_the_cookie() {
        let set = parse_header("SAPISID=secret-value; VISITOR_INFO1_LIVE=v1").unwrap();
        let shown = format!("{set:?}");
        assert!(!shown.contains("secret-value"));
        assert!(shown.contains("[redacted]"));
    }
}
