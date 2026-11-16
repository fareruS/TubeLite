//! Logging with redaction. Never log cookies, API keys, tokens,
//! signed URLs, or IP addresses.
//!
//! Redaction is deliberately conservative: any value that looks like a
//! signed googlevideo URL, a SAPISID / Authorization header, or a long
//! token is replaced before it reaches the log or the diagnostics blob
//! a user may paste into a public issue.

/// Redact a single string value for logging.
pub fn redact(value: &str) -> String {
    // Signed googlevideo URLs carry `signature=` / `sig=` / `s=` params.
    // Never log the full URL — log only the host + a marker.
    if value.contains("googlevideo.com") {
        return "[redacted googlevideo URL]".to_string();
    }
    if value.contains("SAPISIDHASH") || value.contains("Authorization:") {
        return "[redacted auth]".to_string();
    }
    // Long opaque tokens: keep a 4-char prefix for debugging, drop the rest.
    if value.len() > 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        let prefix: String = value.chars().take(4).collect();
        return format!("{prefix}…[redacted token, len={}]", value.len());
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_googlevideo_and_auth() {
        assert_eq!(
            redact("https://r1---sn.googlevideo.com/videoplayback?signature=abc123"),
            "[redacted googlevideo URL]"
        );
        assert_eq!(
            redact("Authorization: SAPISIDHASH 123_abc"),
            "[redacted auth]"
        );
        assert_eq!(redact("hello"), "hello");
    }
}
