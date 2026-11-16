//! Phase 0 redaction: strip personal data from a saved InnerTube response.
//!
//! Reads a raw response (or saved request) JSON file and writes a redacted
//! copy. Never modifies the input.
//!
//! What is stripped (recursive over the whole JSON value):
//! - Exact keys (case-insensitive): cookie, authorization, sapisid,
//!   visitordata, x-goog-visitor-id, sessiontoken, password, accountname,
//!   accountemail, email, accountphoto, remotehost, configinfo,
//!   deviceexperimentid, rollouttoken, x-client-data, signaturecipher,
//!   cipher. Their values become `"REDACTED"`.
//! - Email addresses in any string value -> `"REDACTED@example.invalid"`.
//! - Signed stream URLs: any URL whose host contains googlevideo.com, or any
//!   URL with a `"signature="` query parameter, has its query string removed
//!   (signatures live in the query). Host + path are kept so itag/mime
//!   structure stays visible. A percent-encoded signed URL (e.g. the
//!   `url=` inside a `signatureCipher`, where the whole query is `%3F…%3D…`)
//!   is redacted wholesale — decoding it only to check would still leave a
//!   live signed URL in the output.
//! - Extra sensitive strings listed one per line in the deny-list file (e.g.
//!   your own channel ID, account name): every occurrence is replaced with
//!   `"REDACTED"`.
//!
//! Verification: after redacting, the output is re-scanned for every
//! deny-list string, any email address, and any `"SAPISID="` / `"Cookie:"` /
//! `"Authorization:"` literal, plus any surviving `"expire="` /
//! `"expire%3D"` (a signed stream URL that slipped through). If anything is
//! found, the tool exits non-zero and does NOT write the output file.
//!
//! Usage:
//! ```text
//! redact <input.json> <deny-list.txt> <output.json>
//! ```

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use serde_json::Value;

/// Exact keys (matched case-insensitively) whose values become "REDACTED".
const SENSITIVE_KEYS: &[&str] = &[
    "cookie",
    "authorization",
    "sapisid",
    "visitordata",
    "x-goog-visitor-id",
    "sessiontoken",
    "password",
    "accountname",
    "accountemail",
    "email",
    "accountphoto",
    "remotehost",
    "configinfo",
    "deviceexperimentid",
    "rollouttoken",
    "x-client-data",
    // Cipher blobs are percent-encoded signed stream URLs (the `s=` decipher
    // signature plus a signed `url=`). The itag/mime structure lives in
    // sibling fields, so the whole value is redacted, never kept.
    "signaturecipher",
    "cipher",
];

const FORBIDDEN_LITERALS: &[&str] = &[
    "SAPISID=",
    "Cookie:",
    "Authorization:",
    // Backstop: any surviving signed stream URL. A plain `expire=` cannot
    // survive query-stripping, and an encoded one only lives inside a
    // cipher that the key rule above must have caught.
    "expire=",
    "expire%3D",
];
const EMAIL_PLACEHOLDER: &str = "REDACTED@example.invalid";

/// Counts for the run report.
#[derive(Debug, Default)]
pub struct Stats {
    pub sensitive_keys: usize,
    pub signed_urls: usize,
    pub emails: usize,
    pub extra_strings: usize,
}

// --- email matching (no regex crate) -------------------------------------
//
// The pattern is `[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}`.
// Implemented as a linear byte scan: every byte is examined once, and the
// email check only runs on strings that contain '@' at all, so long token
// strings (continuation tokens, hashes) stay fast.

fn is_local_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_domain_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')
}

/// Find an email match in `s` at or after byte offset `from`.
/// Returns `(start, end)` byte indices of the match, or `None`.
/// All indices fall on ASCII characters, so slicing on them is safe.
pub fn find_email(s: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        // Find the next '@'.
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        let at = i;
        // Local part: maximal run of local chars immediately before '@'.
        let mut local_start = at;
        while local_start > 0 && is_local_char(bytes[local_start - 1]) {
            local_start -= 1;
        }
        if local_start == at {
            i = at + 1;
            continue;
        }
        // Domain run: maximal run of domain chars immediately after '@'.
        let mut domain_end = at + 1;
        while domain_end < bytes.len() && is_domain_char(bytes[domain_end]) {
            domain_end += 1;
        }
        if domain_end == at + 1 {
            i = at + 1;
            continue;
        }
        // The domain must contain a dot followed by >= 2 letters (the TLD).
        // Try dots right-to-left, like a greedy regex backtracking: the
        // match ends after the letter run following the chosen dot.
        let mut dot = domain_end;
        let mut matched: Option<usize> = None;
        while dot > at + 1 {
            dot -= 1;
            if bytes[dot] != b'.' {
                continue;
            }
            let mut tld_end = dot + 1;
            while tld_end < domain_end && bytes[tld_end].is_ascii_alphabetic() {
                tld_end += 1;
            }
            if tld_end - (dot + 1) >= 2 {
                matched = Some(tld_end);
                break;
            }
        }
        if let Some(end) = matched {
            return Some((local_start, end));
        }
        i = at + 1;
    }
    None
}

/// Replace every email address in `s` with the placeholder.
/// Returns the new string and the number of replacements.
pub fn replace_emails(s: &str) -> (String, usize) {
    if !s.contains('@') {
        return (s.to_string(), 0);
    }
    let mut out = String::with_capacity(s.len());
    let mut count = 0;
    let mut cursor = 0;
    while let Some((start, end)) = find_email(s, cursor) {
        out.push_str(&s[cursor..start]);
        out.push_str(EMAIL_PLACEHOLDER);
        count += 1;
        cursor = end;
    }
    out.push_str(&s[cursor..]);
    (out, count)
}

// --- URL stripping --------------------------------------------------------

/// Percent-decode `%XX` sequences for detection only (the output keeps the
/// original encoding). Invalid `%` sequences are left as-is.
fn percent_decode_for_check(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// True when the value is a percent-encoded signed stream URL: decoding it
/// reveals a googlevideo host carrying an expiry or signature parameter,
/// and there is no literal `?` for the plain query-strip to cut at.
/// Such a value is a live signed URL even though no plain `?` or
/// `googlevideo.com` appears in it, so it must be redacted wholesale.
/// A plain signed URL *with* a literal `?` is left to `strip_signed_url`,
/// which removes its query and keeps the harmless host + path.
fn is_percent_encoded_signed_url(value: &str) -> bool {
    if !value.contains('%') || value.contains('?') {
        return false;
    }
    let decoded = percent_decode_for_check(value).to_ascii_lowercase();
    decoded.contains("googlevideo.com")
        && (decoded.contains("expire=")
            || decoded.contains("signature=")
            || decoded.contains("sig="))
}

/// Drop the query string of signed/stream URLs, keep host + path.
/// A URL is "signed" when it mentions googlevideo.com or carries a
/// `signature=` query parameter. A `?` with an empty query is left alone.
pub fn strip_signed_url(value: &str) -> String {
    let lowered = value.to_ascii_lowercase();
    if lowered.contains("googlevideo.com") || lowered.contains("signature=") {
        if let Some(q) = value.find('?') {
            let after = &value[q + 1..];
            let query = after.split('#').next().unwrap_or("");
            if !query.is_empty() {
                return value[..q].to_string();
            }
        }
    }
    value.to_string()
}

// --- recursive redaction --------------------------------------------------

fn redact_string_inner(value: &str, extra: &[String], stats: &mut Stats) -> String {
    // A percent-encoded signed URL is a live secret in disguise: redact the
    // whole value instead of trying to strip a query that isn't visible.
    if is_percent_encoded_signed_url(value) {
        stats.signed_urls += 1;
        return "REDACTED".to_string();
    }
    let original = value;
    let mut value = strip_signed_url(value);
    if value != original {
        stats.signed_urls += 1;
    }
    // Fast pre-filter: the email scan only runs when '@' is present at all.
    if value.contains('@') {
        let (replaced, n) = replace_emails(&value);
        if n > 0 {
            stats.emails += n;
            value = replaced;
        }
    }
    for secret in extra {
        if !secret.is_empty() && value.contains(secret.as_str()) {
            value = value.replace(secret.as_str(), "REDACTED");
            stats.extra_strings += 1;
        }
    }
    value
}

/// Recursively redact a JSON value. Object keys are preserved (except
/// sensitive keys, whose *values* become "REDACTED"); numbers, booleans,
/// and null pass through untouched.
pub fn redact_value(value: Value, extra: &[String], stats: &mut Stats) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, val) in map {
                if SENSITIVE_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                    stats.sensitive_keys += 1;
                    out.insert(key, Value::String("REDACTED".to_string()));
                } else {
                    out.insert(key, redact_value(val, extra, stats));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact_value(item, extra, stats))
                .collect(),
        ),
        Value::String(s) => Value::String(redact_string_inner(&s, extra, stats)),
        other => other,
    }
}

// --- verification ---------------------------------------------------------

/// Re-scan the serialized redacted output. Fails if any deny-list string,
/// any real email address, or any forbidden auth literal survives.
/// The redaction placeholder itself is email-shaped, so it is excluded
/// from the email check — otherwise a file is flagged for its own
/// replacement.
pub fn verify(redacted_text: &str, extra: &[String]) -> Result<(), String> {
    let mut problems: Vec<String> = Vec::new();
    for secret in extra {
        if !secret.is_empty() && redacted_text.contains(secret.as_str()) {
            problems.push("extra sensitive string still present".to_string());
            break;
        }
    }
    let check_text = redacted_text.replace(EMAIL_PLACEHOLDER, "");
    if check_text.contains('@') && find_email(&check_text, 0).is_some() {
        problems.push("email address still present".to_string());
    }
    for literal in FORBIDDEN_LITERALS {
        if redacted_text.contains(literal) {
            problems.push(format!("literal {literal:?} still present"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

// --- entry point ----------------------------------------------------------

/// Read the deny-list file: one sensitive string per line, blanks skipped,
/// longest first (so a longer string is never partially replaced by a
/// shorter one that is its prefix).
fn load_deny_list(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("cannot read deny-list {}: {e}", path.display()))?;
    let mut strings: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    strings.sort_by_key(|a| std::cmp::Reverse(a.len()));
    Ok(strings)
}

/// Redact `input_path` into `output_path` using the deny-list at
/// `deny_path`. The output is written only after verification passes.
/// Returns the redaction stats.
pub fn run(input_path: &Path, deny_path: &Path, output_path: &Path) -> Result<Stats, String> {
    let input_text = fs::read_to_string(input_path)
        .map_err(|e| format!("cannot read input {}: {e}", input_path.display()))?;
    let parsed: Value = serde_json::from_str(&input_text)
        .map_err(|e| format!("invalid input JSON {}: {e}", input_path.display()))?;
    let extra = load_deny_list(deny_path)?;

    let mut stats = Stats::default();
    let redacted = redact_value(parsed, &extra, &mut stats);

    // Verification pass: nothing sensitive may survive in the output text.
    let text = serde_json::to_string(&redacted)
        .map_err(|e| format!("cannot serialize redacted JSON: {e}"))?;
    verify(&text, &extra).map_err(|e| format!("FAILED verification: {e}"))?;

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create output directory {}: {e}", parent.display()))?;
        }
    }
    let pretty = serde_json::to_string_pretty(&redacted)
        .map_err(|e| format!("cannot serialize redacted JSON: {e}"))?;
    fs::write(output_path, format!("{pretty}\n"))
        .map_err(|e| format!("cannot write output {}: {e}", output_path.display()))?;
    Ok(stats)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: redact <input.json> <deny-list.txt> <output.json>");
        return ExitCode::from(2);
    }
    let input_path = Path::new(&args[1]);
    let deny_path = Path::new(&args[2]);
    let out_path = Path::new(&args[3]);
    match run(input_path, deny_path, out_path) {
        Ok(stats) => {
            println!("redacted: {} -> {}", args[1], args[3]);
            println!(
                "stats: sensitive_keys={} signed_urls={} emails={} extra_strings={}",
                stats.sensitive_keys, stats.signed_urls, stats.emails, stats.extra_strings
            );
            println!("verification: passed (no extra strings, emails, or auth literals)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("redact: error: {e}");
            // Verification failures and input errors are both fatal and
            // write nothing; distinguish them by exit code.
            if e.starts_with("FAILED verification") {
                ExitCode::from(1)
            } else {
                ExitCode::from(2)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn redact_json(value: Value, extra: &[String]) -> (Value, Stats) {
        let mut stats = Stats::default();
        let out = redact_value(value, extra, &mut stats);
        (out, stats)
    }

    // --- sensitive keys ---

    #[test]
    fn sensitive_keys_replaced_case_insensitive() {
        let (out, stats) = redact_json(
            json!({
                "cookie": "secret",
                "Cookie": "also-secret",
                "VISITORDATA": "abc",
                "x-goog-visitor-id": "xyz",
                "sessionToken": "tok",
                "accountName": "Some One",
                "accountEmail": "a@b.com",
                "email": "c@d.com",
                "accountPhoto": "https://example.com/pic.jpg",
                "remoteHost": "1.2.3.4",
                "configInfo": {"a": 1},
                "deviceExperimentId": "exp",
                "rolloutToken": "roll",
                "x-client-data": "data",
                "password": "hunter2",
                "authorization": "SAPISIDHASH 1_abc",
                "sapisid": "abc",
                "normal": "keep me",
                "nested": {"Email": "e@f.com", "count": 3, "ok": true, "none": null}
            }),
            &[],
        );
        assert_eq!(out["cookie"], "REDACTED");
        assert_eq!(out["Cookie"], "REDACTED");
        assert_eq!(out["VISITORDATA"], "REDACTED");
        assert_eq!(out["x-goog-visitor-id"], "REDACTED");
        assert_eq!(out["sessionToken"], "REDACTED");
        assert_eq!(out["accountName"], "REDACTED");
        assert_eq!(out["accountEmail"], "REDACTED");
        assert_eq!(out["email"], "REDACTED");
        assert_eq!(out["accountPhoto"], "REDACTED");
        assert_eq!(out["remoteHost"], "REDACTED");
        assert_eq!(out["configInfo"], "REDACTED");
        assert_eq!(out["deviceExperimentId"], "REDACTED");
        assert_eq!(out["rolloutToken"], "REDACTED");
        assert_eq!(out["x-client-data"], "REDACTED");
        assert_eq!(out["password"], "REDACTED");
        assert_eq!(out["authorization"], "REDACTED");
        assert_eq!(out["sapisid"], "REDACTED");
        assert_eq!(out["normal"], "keep me");
        assert_eq!(out["nested"]["Email"], "REDACTED");
        // Numbers, booleans, and null survive untouched.
        assert_eq!(out["nested"]["count"], 3);
        assert_eq!(out["nested"]["ok"], true);
        assert!(out["nested"]["none"].is_null());
        assert_eq!(stats.sensitive_keys, 18);
    }

    #[test]
    fn similar_but_not_sensitive_keys_are_kept() {
        let (out, _) = redact_json(
            json!({"cookies": "keep", "myemail": "keep", "emailAddress": "keep"}),
            &[],
        );
        assert_eq!(out["cookies"], "keep");
        assert_eq!(out["myemail"], "keep");
        assert_eq!(out["emailAddress"], "keep");
    }

    // --- emails ---

    #[test]
    fn email_replaced_in_string_values() {
        let (out, stats) = redact_json(
            json!({"text": "contact me at first.last+tag@example.co.uk any time"}),
            &[],
        );
        assert_eq!(
            out["text"],
            "contact me at REDACTED@example.invalid any time"
        );
        assert_eq!(stats.emails, 1);
    }

    #[test]
    fn email_finder_shape() {
        assert_eq!(find_email("a@b.com", 0), Some((0, 7)));
        assert_eq!(find_email("a@b.co.uk", 0), Some((0, 9)));
        assert_eq!(find_email("a@b.c", 0), None); // TLD needs >= 2 letters
        assert_eq!(find_email("@b.com", 0), None); // no local part
        assert_eq!(find_email("a@b", 0), None); // no dot in domain
        assert_eq!(find_email("a@b.com.", 0), Some((0, 7))); // trailing dot not eaten
        assert_eq!(find_email("xx a@b.com yy c@d.org", 0), Some((3, 10)));
        // Multiple emails are all replaced.
        let (s, n) = replace_emails("a@b.com and c@d.org");
        assert_eq!(s, "REDACTED@example.invalid and REDACTED@example.invalid");
        assert_eq!(n, 2);
    }

    #[test]
    fn long_token_without_at_is_untouched() {
        // The performance trap: a multi-KB token with no '@' must pass
        // through unchanged (and fast — the email scan never runs).
        let token = "A".repeat(50_000);
        let (out, stats) = redact_json(json!({"continuation": token}), &[]);
        assert_eq!(out["continuation"].as_str().unwrap().len(), 50_000);
        assert_eq!(stats.emails, 0);
    }

    // --- deny list ---

    #[test]
    fn deny_list_replaces_everywhere_and_longest_first() {
        let extra = vec!["UC12345".to_string(), "UC123".to_string()];
        let mut sorted = extra.clone();
        sorted.sort_by_key(|a| std::cmp::Reverse(a.len()));
        let (out, stats) = redact_json(
            json!({"channel": "UC12345", "text": "by UC12345 and UC123"}),
            &sorted,
        );
        assert_eq!(out["channel"], "REDACTED");
        assert_eq!(out["text"], "by REDACTED and REDACTED");
        // Counted per value per secret found: "channel" hits UC12345 once,
        // "text" hits UC12345 once and UC123 once.
        assert_eq!(stats.extra_strings, 3);
    }

    // --- signed URLs ---

    #[test]
    fn cipher_keys_are_redacted_whole() {
        // A signatureCipher is a percent-encoded signed stream URL: the
        // `s=` decipher signature plus a signed `url=`. The itag/mime
        // structure lives in sibling fields, so the whole value goes.
        let (out, stats) = redact_json(
            json!({
                "itag": 18,
                "mimeType": "video/mp4",
                "signatureCipher": "s=ABC123&sp=sig&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fexpire%3D1794858780%26sig%3DXYZ",
                "cipher": "s=DEF&url=https%3A%2F%2Frr2.googlevideo.com%2Fp%3Fexpire%3D1",
            }),
            &[],
        );
        assert_eq!(out["itag"], 18);
        assert_eq!(out["mimeType"], "video/mp4");
        assert_eq!(out["signatureCipher"], "REDACTED");
        assert_eq!(out["cipher"], "REDACTED");
        assert_eq!(stats.sensitive_keys, 2);
    }

    #[test]
    fn percent_encoded_signed_url_is_redacted_whole() {
        // No cipher key here — the bare string value itself is a
        // percent-encoded signed URL. Plain-text checks for "?" and
        // "googlevideo.com" cannot see it, so the decoded check must.
        let encoded = "s=ABC&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fexpire%3D1794858780%26lsig%3DXYZ";
        let (out, stats) = redact_json(json!({"blob": encoded}), &[]);
        assert_eq!(out["blob"], "REDACTED");
        assert_eq!(stats.signed_urls, 1);
        // ...but an encoded URL with no expiry/signature is left alone.
        let harmless = "https%3A%2F%2Fexample.com%2Fwatch%3Fv%3Dabc";
        let (out, _) = redact_json(json!({"link": harmless}), &[]);
        assert_eq!(out["link"], harmless);
        // ...and a plain signed URL *with* a literal `?` keeps its host +
        // path — the query strip handles it, no wholesale redaction.
        let plain = "https://rr1.googlevideo.com/videoplayback?expire=1794858780&sig=XYZ";
        let (out, stats) = redact_json(json!({"u": plain}), &[]);
        assert_eq!(out["u"], "https://rr1.googlevideo.com/videoplayback");
        assert_eq!(stats.signed_urls, 1);
    }

    #[test]
    fn verification_fails_on_surviving_expire() {
        for text in [
            r#"{"u": "https://x.googlevideo.com/p?expire=123"}"#,
            r#"{"u": "s=1&url=https%3A%2F%2Fx.googlevideo.com%2Fp%3Fexpire%3D123"}"#,
        ] {
            let err = verify(text, &[]).unwrap_err();
            assert!(err.contains("expire"), "err: {err}");
        }
    }

    #[test]
    fn signed_urls_stripped() {
        assert_eq!(
            strip_signed_url(
                "https://rr1---sn-abc.googlevideo.com/videoplayback?expire=123&sig=ABC&itag=251"
            ),
            "https://rr1---sn-abc.googlevideo.com/videoplayback"
        );
        assert_eq!(
            strip_signed_url("https://example.com/watch?v=abc&signature=XYZ"),
            "https://example.com/watch"
        );
        // Normal URLs and empty queries are untouched.
        assert_eq!(
            strip_signed_url("https://www.youtube.com/watch?v=abc123"),
            "https://www.youtube.com/watch?v=abc123"
        );
        assert_eq!(
            strip_signed_url("https://rr1.googlevideo.com/videoplayback?"),
            "https://rr1.googlevideo.com/videoplayback?"
        );
        let (out, stats) = redact_json(
            json!({"url": "https://x.googlevideo.com/p?sig=1", "plain": "https://example.com/a?b=c"}),
            &[],
        );
        assert_eq!(out["url"], "https://x.googlevideo.com/p");
        assert_eq!(out["plain"], "https://example.com/a?b=c");
        assert_eq!(stats.signed_urls, 1);
    }

    // --- verification ---

    #[test]
    fn verification_passes_for_clean_output() {
        let (out, _) = redact_json(json!({"a": "hello", "b": "x@y.zz"}), &[]);
        let text = serde_json::to_string(&out).unwrap();
        // The placeholder itself must not trip the email check.
        assert!(text.contains("REDACTED@example.invalid"));
        assert!(verify(&text, &[]).is_ok());
    }

    #[test]
    fn verification_fails_on_deny_string_in_key() {
        // A deny-list string hiding in a JSON *key* is not replaced by the
        // value walk, so verification must catch it.
        let text = r#"{"UC12345": "value"}"#;
        let err = verify(text, &["UC12345".to_string()]).unwrap_err();
        assert!(err.contains("extra sensitive string"), "err: {err}");
    }

    #[test]
    fn verification_fails_on_email_in_key() {
        let text = r#"{"owner@real.com": "value"}"#;
        let err = verify(text, &[]).unwrap_err();
        assert!(err.contains("email"), "err: {err}");
    }

    #[test]
    fn verification_fails_on_auth_literals() {
        for literal in ["SAPISID=abc", "Cookie: abc", "Authorization: Bearer x"] {
            let text = serde_json::to_string(&json!({"note": literal})).unwrap();
            let err = verify(&text, &[]).unwrap_err();
            assert!(err.contains("literal"), "err: {err}");
        }
    }

    // --- end-to-end run() ---

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("redact-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn run_end_to_end_writes_pretty_json_and_keeps_structure() {
        let dir = temp_dir("ok");
        let input = dir.join("in.json");
        let deny = dir.join("deny.txt");
        let output = dir.join("out.json");
        fs::write(
            &input,
            serde_json::to_string(&json!({
                "contents": [{"videoId": "abc", "count": 3, "ok": true, "x": null}],
                "cookie": "secret",
                "owner": "reach me at me@example.com",
                "url": "https://r.googlevideo.com/p?sig=1",
                "channelId": "UC99999"
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(&deny, "UC99999\n\n").unwrap();
        let stats = run(&input, &deny, &output).unwrap();
        assert_eq!(stats.sensitive_keys, 1);
        assert_eq!(stats.emails, 1);
        assert_eq!(stats.signed_urls, 1);
        assert_eq!(stats.extra_strings, 1);
        let text = fs::read_to_string(&output).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["cookie"], "REDACTED");
        assert_eq!(parsed["contents"][0]["count"], 3);
        assert_eq!(parsed["contents"][0]["ok"], true);
        assert!(parsed["contents"][0]["x"].is_null());
        assert_eq!(parsed["owner"], "reach me at REDACTED@example.invalid");
        assert_eq!(parsed["url"], "https://r.googlevideo.com/p");
        assert_eq!(parsed["channelId"], "REDACTED");
        assert!(text.ends_with('\n'));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_verification_failure_writes_nothing() {
        let dir = temp_dir("fail");
        let input = dir.join("in.json");
        let deny = dir.join("deny.txt");
        let output = dir.join("out.json");
        // The deny string sits in a key, so the value walk cannot remove it.
        fs::write(&input, r#"{"UC99999": "value"}"#).unwrap();
        fs::write(&deny, "UC99999\n").unwrap();
        let err = run(&input, &deny, &output).unwrap_err();
        assert!(err.contains("FAILED verification"), "err: {err}");
        assert!(!output.exists(), "output must not be written on failure");
        let _ = fs::remove_dir_all(&dir);
    }
}
