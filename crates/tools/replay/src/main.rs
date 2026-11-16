//! Phase 0 replay tool: re-send a saved InnerTube request with fresh auth.
//!
//! Reads a saved request file (JSON: method, url, headers, body), takes the
//! user's browser cookies from a local file, computes a fresh SAPISIDHASH for
//! the current time, and sends the request with Cookie + Authorization
//! headers. The response body is written verbatim to the output path.
//!
//! Secrets policy:
//! - The cookie file and the request file's Cookie/Authorization headers are
//!   NEVER printed, logged, or written to the output.
//! - The computed SAPISIDHASH is itself a usable auth token for its window:
//!   it is never printed either, only that it was set.
//! - The output must be redacted (see the `redact` binary) before it goes
//!   anywhere outside a gitignored directory.
//!
//! Request file format (example):
//! ```json
//! {
//!   "method": "POST",
//!   "url": "https://www.youtube.com/youtubei/v1/browse?prettyPrint=false",
//!   "headers": { "Content-Type": "application/json", "Origin": "https://www.youtube.com" },
//!   "body": {"context": {"client": {"clientName": "WEB"}}}
//! }
//! ```
//! Cookie / Authorization headers in the request file, if any, are IGNORED
//! and replaced with freshly computed ones. No endpoint, browseId, or client
//! version is hardcoded here — everything comes from the template file.
//!
//! Usage:
//! ```text
//! replay <template.json> <cookies.txt> <output.json>
//! ```

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, Url};
use serde::Deserialize;
use sha1::{Digest, Sha1};

/// Origin used in the SAPISIDHASH computation (and sent as the Origin header
/// by the template itself).
const ORIGIN: &str = "https://www.youtube.com";

/// Headers that must never be re-sent from a capture: HTTP/2 pseudo-headers
/// (":authority", ":method", ...) and ones the HTTP client manages or that
/// belong to the original response/transport, not the request. accept-encoding
/// is dropped too, so the server answers plain JSON instead of Brotli.
/// x-goog-authuser names the signed-in account index from the capture. If the
/// cookies came from a different account, a stale index points the server at
/// the wrong account; the cookies alone identify it.
const DROP_HEADERS: &[&str] = &[
    "content-length",
    "content-encoding",
    "accept-encoding",
    "transfer-encoding",
    "connection",
    "host",
    "x-goog-authuser",
];

/// A saved request template, exactly like `fixtures/raw/*.request.json`.
#[derive(Debug, Deserialize)]
struct RequestTemplate {
    /// HTTP method; defaults to POST when absent.
    #[serde(default = "default_method")]
    method: String,
    url: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    body: Option<serde_json::Value>,
}

fn default_method() -> String {
    "POST".to_string()
}

/// Return `"<ts>_<sha1_hex('<ts> <sapisid> <origin>')>"` (InnerTube auth).
pub fn sapisid_hash(sapisid: &str, origin: &str, timestamp_unix: u64) -> String {
    let input = format!("{timestamp_unix} {sapisid} {origin}");
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    // sha1 0.11's digest type has no LowerHex impl; encode bytes manually.
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{timestamp_unix}_{hex}")
}

/// Read cookies from a file, as a Cookie header string.
///
/// Accepts either a raw `"name=value; name2=value2"` header, or a Netscape
/// cookies.txt export (the "Get cookies.txt LOCALLY" extension format):
/// tab-separated lines `"domain flag path secure expiry name value"`.
/// For Netscape files only cookies that apply to `host` are kept, so an
/// export covering every site still sends just this site's cookies.
/// Exports can list the same cookie several times (multiple
/// stores/profiles) with different values — a browser never sends that, and
/// conflicting values break session binding, so the first occurrence of each
/// name is kept.
pub fn load_cookie_header(text: &str, host: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("cookie file is empty".to_string());
    }
    if text.starts_with("# Netscape") {
        let mut pairs: Vec<String> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 7 {
                continue;
            }
            let domain = fields[0];
            let name = fields[5];
            let value = fields[6];
            let base = domain.trim_start_matches('.');
            if !host.is_empty() && !(host == base || host.ends_with(&format!(".{base}"))) {
                continue;
            }
            if !seen.insert(name) {
                continue;
            }
            pairs.push(format!("{name}={value}"));
        }
        if pairs.is_empty() {
            return Err(format!("no cookies for {} in Netscape file", host));
        }
        return Ok(pairs.join("; "));
    }
    // Tolerate a pasted "Cookie: ..." line.
    let text = text
        .strip_prefix("Cookie:")
        .or_else(|| text.strip_prefix("cookie:"))
        .map(str::trim)
        .unwrap_or(text);
    // Tolerate a multi-line paste: join continuation lines.
    Ok(text
        .lines()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string())
}

/// Extract the SAPISID cookie value from a Cookie header string, falling
/// back to `__Secure-3PAPISID` when SAPISID itself is absent.
pub fn extract_sapisid(cookie_header: &str) -> Option<String> {
    for wanted in ["SAPISID", "__Secure-3PAPISID"] {
        for part in cookie_header.split(';') {
            let part = part.trim();
            if let Some((name, value)) = part.split_once('=') {
                if name.trim() == wanted {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

/// Request headers with any stored Cookie/Authorization replaced by fresh
/// values. Pseudo-headers, transport headers, and the account-index header
/// are dropped (see [`DROP_HEADERS`]).
pub fn build_headers(
    template_headers: &HashMap<String, String>,
    cookie_header: &str,
    auth_value: &str,
) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    for (name, value) in template_headers {
        let lower = name.to_ascii_lowercase();
        if lower.starts_with(':')
            || lower == "cookie"
            || lower == "authorization"
            || DROP_HEADERS.contains(&lower.as_str())
        {
            continue;
        }
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("invalid header name {name:?}: {e}"))?;
        let value = HeaderValue::from_str(value)
            .map_err(|e| format!("invalid header value for {name}: {e}"))?;
        headers.insert(name, value);
    }
    headers.insert(
        "cookie",
        HeaderValue::from_str(cookie_header)
            .map_err(|e| format!("invalid Cookie header value: {e}"))?,
    );
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("SAPISIDHASH {auth_value}"))
            .map_err(|e| format!("invalid Authorization header value: {e}"))?,
    );
    Ok(headers)
}

/// Send the templated request with fresh auth. Returns `(status, byte_count)`.
/// The response body is written verbatim to `out_path` (parent directories
/// are created). Nothing secret is printed or returned.
pub async fn run(
    template_path: &Path,
    cookies_path: &Path,
    out_path: &Path,
) -> Result<(u16, usize), String> {
    let template_text = fs::read_to_string(template_path).map_err(|e| {
        format!(
            "cannot read request template {}: {e}",
            template_path.display()
        )
    })?;
    let template: RequestTemplate = serde_json::from_str(&template_text)
        .map_err(|e| format!("invalid request template {}: {e}", template_path.display()))?;
    let url = Url::parse(&template.url)
        .map_err(|e| format!("invalid request url {:?}: {e}", template.url))?;
    let host = url.host_str().unwrap_or("").to_string();

    let cookie_text = fs::read_to_string(cookies_path)
        .map_err(|e| format!("cannot read cookie file {}: {e}", cookies_path.display()))?;
    let cookie_header = load_cookie_header(&cookie_text, &host)?;
    let sapisid = extract_sapisid(&cookie_header)
        .ok_or_else(|| "no SAPISID cookie found in cookie file".to_string())?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("system clock is before the epoch: {e}"))?
        .as_secs();
    let auth_value = sapisid_hash(&sapisid, ORIGIN, timestamp);
    let headers = build_headers(&template.headers, &cookie_header, &auth_value)?;

    let method = Method::from_bytes(template.method.as_bytes())
        .map_err(|e| format!("invalid request method {:?}: {e}", template.method))?;
    let client = innertube::http_client(Duration::from_secs(30))
        .map_err(|e| format!("cannot build HTTP client: {e}"))?;
    let mut request = client.request(method, url).headers(headers);
    if let Some(body) = &template.body {
        let body_text =
            serde_json::to_string(body).map_err(|e| format!("cannot serialize body: {e}"))?;
        request = request.body(body_text);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("network error: {e}"))?;
    let status = response.status().as_u16();
    let raw = response
        .bytes()
        .await
        .map_err(|e| format!("cannot read response body: {e}"))?;

    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create output directory {}: {e}", parent.display()))?;
        }
    }
    fs::write(out_path, &raw)
        .map_err(|e| format!("cannot write output {}: {e}", out_path.display()))?;
    Ok((status, raw.len()))
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: replay <template.json> <cookies.txt> <output.json>");
        return ExitCode::from(2);
    }
    let template_path = Path::new(&args[1]);
    let cookies_path = Path::new(&args[2]);
    let out_path = Path::new(&args[3]);
    match run(template_path, cookies_path, out_path).await {
        Ok((status, bytes)) => {
            // Status + byte count only. Never a cookie, SAPISID, or hash.
            println!("status: {status}");
            println!("bytes: {bytes}");
            if (200..300).contains(&status) {
                ExitCode::SUCCESS
            } else {
                eprintln!("replay: HTTP {status} (body saved)");
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("replay: error: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    // --- sapisid_hash ---

    #[test]
    fn sapisid_hash_known_vector() {
        // Computed with Python hashlib.sha1("1700000000 test_sapisid https://www.youtube.com").
        assert_eq!(
            sapisid_hash("test_sapisid", "https://www.youtube.com", 1_700_000_000),
            "1700000000_34562bc6921a7e4218ebf0fdb6bd88a734cd8ae8"
        );
    }

    #[test]
    fn sapisid_hash_shape() {
        // sapisid_hash itself returns "<epoch>_<40 hex>" (no prefix); the
        // "SAPISIDHASH " scheme is added when building the header.
        let h = sapisid_hash("x", "https://www.youtube.com", 1);
        let (epoch, hex) = h.split_once('_').unwrap();
        assert_eq!(epoch, "1");
        assert!(epoch.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(hex.len(), 40);
        assert!(hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        // The full Authorization value is "SAPISIDHASH <epoch>_<40 hex>".
        let auth = format!("SAPISIDHASH {h}");
        assert!(auth.starts_with("SAPISIDHASH "));
        let rest = auth.strip_prefix("SAPISIDHASH ").unwrap();
        assert_eq!(rest, h);
    }

    // --- cookie file parsing ---

    const NETSCAPE_SAMPLE: &str = "# Netscape HTTP Cookie File\n\
        .youtube.com\tTRUE\t/\tTRUE\t1800000000\tSAPISID\tsapisid-value\n\
        .youtube.com\tTRUE\t/\tTRUE\t1800000000\tVISITOR_INFO1_LIVE\tvisitor-value\n\
        .example.com\tTRUE\t/\tFALSE\t1800000000\tSAPISID\tother-site-value\n\
        .youtube.com\tTRUE\t/\tTRUE\t1800000000\tSAPISID\tduplicate-should-lose\n";

    #[test]
    fn netscape_filters_to_host_and_dedupes_first_wins() {
        let header = load_cookie_header(NETSCAPE_SAMPLE, "www.youtube.com").unwrap();
        assert_eq!(
            header,
            "SAPISID=sapisid-value; VISITOR_INFO1_LIVE=visitor-value"
        );
    }

    #[test]
    fn netscape_no_cookies_for_host_errors() {
        assert!(load_cookie_header(NETSCAPE_SAMPLE, "other.example").is_err());
    }

    #[test]
    fn netscape_empty_file_errors() {
        assert!(load_cookie_header("", "www.youtube.com").is_err());
        assert!(load_cookie_header("  \n", "www.youtube.com").is_err());
    }

    #[test]
    fn raw_cookie_header_is_tolerated() {
        let header = load_cookie_header("SAPISID=abc; SID=def", "www.youtube.com").unwrap();
        assert_eq!(header, "SAPISID=abc; SID=def");
        let header = load_cookie_header("Cookie: SAPISID=abc", "www.youtube.com").unwrap();
        assert_eq!(header, "SAPISID=abc");
    }

    #[test]
    fn extract_sapisid_prefers_sapisid_over_3papisid() {
        assert_eq!(
            extract_sapisid("SAPISID=plain; __Secure-3PAPISID=third").as_deref(),
            Some("plain")
        );
        assert_eq!(
            extract_sapisid("__Secure-3PAPISID=third").as_deref(),
            Some("third")
        );
        // __Secure-3PAPISID must not be mistaken for SAPISID.
        assert_eq!(
            extract_sapisid("SID=x; __Secure-3PAPISID=third").as_deref(),
            Some("third")
        );
        assert_eq!(extract_sapisid("SID=x"), None);
    }

    // --- header building ---

    #[test]
    fn build_headers_drops_capture_only_headers_and_replaces_auth() {
        let mut template = HashMap::new();
        template.insert(":authority".to_string(), "www.youtube.com".to_string());
        template.insert(":method".to_string(), "POST".to_string());
        template.insert("content-type".to_string(), "application/json".to_string());
        template.insert("origin".to_string(), "https://www.youtube.com".to_string());
        template.insert("content-length".to_string(), "123".to_string());
        template.insert("accept-encoding".to_string(), "gzip, br".to_string());
        template.insert("x-goog-authuser".to_string(), "0".to_string());
        template.insert("cookie".to_string(), "STALE=1".to_string());
        template.insert("Authorization".to_string(), "STALE".to_string());
        let headers = build_headers(&template, "SAPISID=fresh", "1_deadbeef").unwrap();
        let names: Vec<String> = headers.keys().map(|k| k.as_str().to_string()).collect();
        assert!(names.contains(&"content-type".to_string()));
        assert!(names.contains(&"origin".to_string()));
        assert!(!names.iter().any(|n| n.starts_with(':')));
        assert!(!names.contains(&"content-length".to_string()));
        assert!(!names.contains(&"accept-encoding".to_string()));
        assert!(!names.contains(&"x-goog-authuser".to_string()));
        assert_eq!(headers.get("cookie").unwrap(), "SAPISID=fresh");
        assert_eq!(
            headers.get("authorization").unwrap(),
            "SAPISIDHASH 1_deadbeef"
        );
    }

    // --- mock-server integration test (no real network) ---

    /// One-shot mock HTTP server: records the request headers it received,
    /// then answers with a fixed JSON body.
    async fn serve_once(
        listener: TcpListener,
        captured: Arc<Mutex<Option<HashMap<String, String>>>>,
        body: &'static [u8],
    ) {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        // Read until the header block is complete, then the body per
        // Content-Length.
        let header_end;
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0, "mock server: client closed the connection early");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                header_end = pos + 4;
                break;
            }
        }
        let header_text = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let mut content_length = 0usize;
        let mut headers = HashMap::new();
        for line in header_text.lines().skip(1) {
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
                if name.trim().eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
        }
        while buf.len() < header_end + content_length {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0, "mock server: body truncated");
            buf.extend_from_slice(&chunk[..n]);
        }
        *captured.lock().unwrap() = Some(headers);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.write_all(body).await.unwrap();
        socket.flush().await.unwrap();
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("replay-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn replay_against_mock_server() {
        let dir = temp_dir("mock");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let captured: Arc<Mutex<Option<HashMap<String, String>>>> = Arc::new(Mutex::new(None));
        let server = tokio::spawn(serve_once(
            listener,
            Arc::clone(&captured),
            br#"{"contents": [], "responseContext": {}}"#,
        ));

        // Synthetic Netscape cookie file: the SAPISID cookie is scoped to
        // the mock server's host (127.0.0.1), exactly as production cookies
        // are scoped to www.youtube.com; a youtube.com cookie and an
        // other-domain cookie must both be filtered out for this host.
        let cookies_path = dir.join("cookies.txt");
        let mut cookies = fs::File::create(&cookies_path).unwrap();
        writeln!(cookies, "# Netscape HTTP Cookie File").unwrap();
        writeln!(
            cookies,
            ".youtube.com\tTRUE\t/\tTRUE\t1800000000\tSAPISID\twrong-host-value"
        )
        .unwrap();
        writeln!(
            cookies,
            ".127.0.0.1\tTRUE\t/\tFALSE\t1800000000\tSAPISID\ttest-sapisid-value"
        )
        .unwrap();
        writeln!(
            cookies,
            ".example.com\tTRUE\t/\tFALSE\t1800000000\tEVIL\tnot-for-youtube"
        )
        .unwrap();
        drop(cookies);

        // Template pointing at the mock server (nothing hardcoded about
        // youtubei here — the URL comes from the template, as in production).
        let template_path = dir.join("template.json");
        let template = serde_json::json!({
            "method": "POST",
            "url": format!("http://127.0.0.1:{port}/youtubei/v1/browse?prettyPrint=false"),
            "headers": {
                "content-type": "application/json",
                "origin": "https://www.youtube.com",
                ":authority": "127.0.0.1",
                "content-length": "999",
                "x-goog-authuser": "0"
            },
            "body": {"context": {"client": {"clientName": "WEB"}}}
        });
        fs::write(&template_path, serde_json::to_string(&template).unwrap()).unwrap();

        let out_path = dir.join("out.json");
        let (status, bytes) = run(&template_path, &cookies_path, &out_path).await.unwrap();
        assert_eq!(status, 200);
        assert_eq!(
            fs::read(&out_path).unwrap(),
            br#"{"contents": [], "responseContext": {}}"#
        );
        assert_eq!(bytes, br#"{"contents": [], "responseContext": {}}"#.len());

        server.await.unwrap();
        let headers = captured.lock().unwrap().take().unwrap();
        let auth = headers
            .get("authorization")
            .expect("Authorization header missing");
        assert!(auth.starts_with("SAPISIDHASH "), "auth: {auth}");
        let (_, hash) = auth.split_once(' ').unwrap();
        let (epoch, hex) = hash.split_once('_').unwrap();
        assert!(epoch.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(hex.len(), 40);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        let cookie = headers.get("cookie").expect("Cookie header missing");
        assert!(cookie.contains("SAPISID=test-sapisid-value"));
        assert!(
            !cookie.contains("EVIL"),
            "other-domain cookie leaked: {cookie}"
        );
        assert!(!headers.contains_key("x-goog-authuser"));
        // reqwest sets its own correct Content-Length; the template's
        // stale "999" must not survive.
        assert_ne!(
            headers.get("content-length").map(String::as_str),
            Some("999")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn replay_missing_sapisid_errors() {
        let dir = temp_dir("no-sapisid");
        let template_path = dir.join("template.json");
        fs::write(
            &template_path,
            serde_json::to_string(&serde_json::json!({
                "method": "POST",
                "url": "http://127.0.0.1:9/youtubei/v1/browse?prettyPrint=false",
                "headers": {},
                "body": {}
            }))
            .unwrap(),
        )
        .unwrap();
        let cookies_path = dir.join("cookies.txt");
        fs::write(&cookies_path, "SID=only-a-session-cookie\n").unwrap();
        let err = run(&template_path, &cookies_path, &dir.join("out.json"))
            .await
            .unwrap_err();
        assert!(err.contains("SAPISID"), "error: {err}");
        let _ = fs::remove_dir_all(&dir);
    }
}
