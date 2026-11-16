//! `SAPISIDHASH`: `"<epoch>_<sha1_hex('<epoch> <SAPISID> <origin>')>"`,
//! sent as `Authorization: SAPISIDHASH <value>`. The epoch is the
//! current unix time in seconds; a fresh value is computed for every
//! request (and every retry) so a refreshed cookie is always used.
//!
//! Known vector (matches the Python replay tool and the old
//! `yt-source` transport): `sapisid_hash("test_sapisid",
//! "https://www.youtube.com", 1700000000)` is
//! `1700000000_34562bc6921a7e4218ebf0fdb6bd88a734cd8ae8`.

use sha1::{Digest, Sha1};

use crate::INNERTUBE_ORIGIN;

/// Compute the SAPISIDHASH value for a SAPISID, origin, and unix
/// epoch (seconds). The returned string does NOT include the
/// `SAPISIDHASH ` prefix.
pub fn sapisid_hash(sapisid: &str, origin: &str, epoch_unix: u64) -> String {
    let input = format!("{epoch_unix} {sapisid} {origin}");
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{epoch_unix}_{hex}")
}

/// The full `Authorization` header value for the session's SAPISID at
/// the current time, or `None` when the session has no SAPISID.
pub fn authorization_value(sapisid: &str) -> String {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "SAPISIDHASH {}",
        sapisid_hash(sapisid, INNERTUBE_ORIGIN, epoch)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sapisid_hash_known_vector() {
        assert_eq!(
            sapisid_hash("test_sapisid", "https://www.youtube.com", 1_700_000_000),
            "1700000000_34562bc6921a7e4218ebf0fdb6bd88a734cd8ae8"
        );
    }

    #[test]
    fn sapisid_hash_shape() {
        // 'SAPISIDHASH ' prefix is added by the caller; the value itself
        // is "<integer epoch>_<40 lowercase hex chars>".
        let value = sapisid_hash("x", "https://www.youtube.com", 1_700_000_001);
        let (epoch, hex) = value.split_once('_').expect("underscore separator");
        assert!(epoch.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(hex.len(), 40);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(hex.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn authorization_value_has_prefix_and_shape() {
        let header = authorization_value("test_sapisid");
        let value = header.strip_prefix("SAPISIDHASH ").expect("prefix");
        let (epoch, hex) = value.split_once('_').expect("underscore separator");
        assert!(epoch.parse::<u64>().is_ok());
        assert_eq!(hex.len(), 40);
    }
}
