//! Integration tests for `Innertube::post` against a scripted local
//! mock server (a raw `TcpListener`, no new dependencies). The VM's
//! egress hangs on youtubei POSTs, so no test touches youtube.com.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use innertube::{Clients, Innertube, Session, SharedSession};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One scripted behaviour for the next connection.
#[derive(Clone)]
enum Behavior {
    /// Close the connection without responding (transport error).
    Close,
    /// Accept and never respond (client timeout).
    Hang,
    /// Respond with a status, headers, and body.
    Respond {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    },
}

struct Mock {
    addr: SocketAddr,
    /// Requests fully read so far.
    requests: Arc<AtomicUsize>,
    /// Authorization headers seen, in order (values redacted to a
    /// presence marker — never store a real hash).
    auth_seen: Arc<Mutex<Vec<bool>>>,
    /// Cookie headers seen, in order (presence only).
    cookie_seen: Arc<Mutex<Vec<bool>>>,
}

impl Mock {
    async fn start(behaviors: Vec<Behavior>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let auth_seen = Arc::new(Mutex::new(Vec::new()));
        let cookie_seen = Arc::new(Mutex::new(Vec::new()));
        let queue = Arc::new(Mutex::new(VecDeque::from(behaviors)));
        let r = requests.clone();
        let a = auth_seen.clone();
        let k = cookie_seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let behavior = queue.lock().unwrap().pop_front().unwrap_or(Behavior::Close);
                let r = r.clone();
                let a = a.clone();
                let k = k.clone();
                tokio::spawn(async move {
                    // Read the request head + body (Content-Length).
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let mut content_length = 0usize;
                    let mut head_done = false;
                    loop {
                        let Ok(n) = stream.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if !head_done {
                            if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                                let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                                for line in head.lines().skip(1) {
                                    if let Some((name, value)) = line.split_once(':') {
                                        let name = name.trim().to_ascii_lowercase();
                                        let value = value.trim();
                                        if name == "content-length" {
                                            content_length = value.parse().unwrap_or(0);
                                        }
                                        if name == "authorization" {
                                            a.lock()
                                                .unwrap()
                                                .push(value.starts_with("SAPISIDHASH "));
                                        }
                                        if name == "cookie" {
                                            k.lock().unwrap().push(!value.is_empty());
                                        }
                                    }
                                }
                                head_done = true;
                                if buf.len() >= pos + 4 + content_length {
                                    break;
                                }
                            }
                        } else if buf.len()
                            >= buf
                                .windows(4)
                                .position(|w| w == b"\r\n\r\n")
                                .map(|p| p + 4 + content_length)
                                .unwrap_or(usize::MAX)
                        {
                            break;
                        }
                    }
                    r.fetch_add(1, Ordering::SeqCst);
                    match behavior {
                        Behavior::Close => {}
                        Behavior::Hang => {
                            tokio::time::sleep(Duration::from_secs(30)).await;
                        }
                        Behavior::Respond {
                            status,
                            headers,
                            body,
                        } => {
                            let reason = match status {
                                200 => "OK",
                                401 => "Unauthorized",
                                403 => "Forbidden",
                                _ => "Status",
                            };
                            let mut out = format!(
                                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                                body.len()
                            );
                            for (name, value) in headers {
                                out.push_str(&format!("{name}: {value}\r\n"));
                            }
                            out.push_str("\r\n");
                            out.push_str(&body);
                            let _ = stream.write_all(out.as_bytes()).await;
                        }
                    }
                });
            }
        });
        Mock {
            addr,
            requests,
            auth_seen,
            cookie_seen,
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn signed_in_session() -> SharedSession {
    SharedSession::new(Session {
        cookie: Some("SAPISID=test_sapisid; SID=1".to_string()),
        ..Default::default()
    })
}

fn client_for(mock: &Mock) -> Innertube {
    Innertube::with_timeouts(
        Clients::embedded().unwrap(),
        signed_in_session(),
        Duration::from_millis(200),
        Duration::from_secs(2),
    )
    .unwrap()
    .with_base(format!("http://{}/youtubei/v1", mock.addr))
    .with_retry_backoff(Duration::from_millis(10))
}

#[tokio::test]
async fn retries_on_timeout_up_to_three_attempts() {
    let mock = Mock::start(vec![
        Behavior::Hang,
        Behavior::Hang,
        Behavior::Respond {
            status: 200,
            headers: vec![],
            body: r#"{"ok":true}"#.to_string(),
        },
    ])
    .await;
    let client = client_for(&mock);
    let value = client
        .post("browse", "WEB", json!({"context": {}}))
        .await
        .expect("third attempt succeeds");
    assert_eq!(value["ok"], true);
    assert_eq!(mock.requests.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn connect_refused_is_retried_then_reported() {
    // No server on this port: every attempt fails to connect.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let client = Innertube::with_timeouts(
        Clients::embedded().unwrap(),
        signed_in_session(),
        Duration::from_millis(200),
        Duration::from_secs(2),
    )
    .unwrap()
    .with_base(format!("http://{addr}/youtubei/v1"))
    .with_retry_backoff(Duration::from_millis(50));
    let start = std::time::Instant::now();
    let err = client
        .post("browse", "WEB", json!({"context": {}}))
        .await
        .expect_err("connect refused must fail");
    // 3 attempts with 50 ms + 100 ms backoff: at least ~150 ms spent.
    assert!(start.elapsed() >= Duration::from_millis(140));
    assert!(matches!(err, innertube::Error::Http(_)));
}

#[tokio::test]
async fn set_cookie_headers_merge_and_deletions_are_ignored() {
    let mock = Mock::start(vec![Behavior::Respond {
        status: 200,
        headers: vec![
            ("Set-Cookie".to_string(), "VISITOR=abc; Path=/".to_string()),
            (
                "Set-Cookie".to_string(),
                "SID=; Expires=Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
            ),
        ],
        body: r#"{"ok":true}"#.to_string(),
    }])
    .await;
    let session = signed_in_session();
    let client = Innertube::with_timeouts(
        Clients::embedded().unwrap(),
        session.clone(),
        Duration::from_millis(200),
        Duration::from_secs(2),
    )
    .unwrap()
    .with_base(format!("http://{}/youtubei/v1", mock.addr));
    client
        .post("browse", "WEB", json!({"context": {}}))
        .await
        .expect("200");
    let cookie = session.cookie().await.expect("cookie kept");
    assert!(cookie.contains("SAPISID=test_sapisid"), "{cookie}");
    assert!(cookie.contains("SID=1"), "{cookie}");
    assert!(cookie.contains("VISITOR=abc"), "{cookie}");
}

#[tokio::test]
async fn signed_in_requests_carry_cookie_and_sapisidhash() {
    let mock = Mock::start(vec![Behavior::Respond {
        status: 200,
        headers: vec![],
        body: r#"{"ok":true}"#.to_string(),
    }])
    .await;
    let client = client_for(&mock);
    client
        .post("browse", "WEB", json!({"context": {}}))
        .await
        .expect("200");
    assert_eq!(mock.auth_seen.lock().unwrap().as_slice(), [true]);
    assert_eq!(mock.cookie_seen.lock().unwrap().as_slice(), [true]);
}

#[tokio::test]
async fn heal_retries_once_after_a_fresh_cookie() {
    let mock = Mock::start(vec![
        Behavior::Respond {
            status: 401,
            headers: vec![],
            body: r#"{"error":{"code":401}}"#.to_string(),
        },
        Behavior::Respond {
            status: 200,
            headers: vec![],
            body: r#"{"ok":true}"#.to_string(),
        },
    ])
    .await;
    let session = signed_in_session();
    let client = Innertube::with_timeouts(
        Clients::embedded().unwrap(),
        session.clone(),
        Duration::from_millis(200),
        Duration::from_secs(2),
    )
    .unwrap()
    .with_base(format!("http://{}/youtubei/v1", mock.addr))
    .with_heal_callback(std::sync::Arc::new(|session: SharedSession| {
        Box::pin(async move {
            // The healer re-mints the cookie and stores it; the
            // notification is what the waiting post() listens for.
            session
                .set_cookie(Some("SAPISID=fresh; SID=2".to_string()))
                .await;
        })
    }))
    .await;
    let value = client
        .post("browse", "WEB", json!({"context": {}}))
        .await
        .expect("heal then retry succeeds");
    assert_eq!(value["ok"], true);
    assert_eq!(mock.requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        session.cookie().await.as_deref(),
        Some("SAPISID=fresh; SID=2")
    );
}

#[tokio::test]
async fn heal_timeout_becomes_session_expired() {
    let mock = Mock::start(vec![Behavior::Respond {
        status: 401,
        headers: vec![],
        body: r#"{"error":{"code":401}}"#.to_string(),
    }])
    .await;
    let client = Innertube::with_timeouts(
        Clients::embedded().unwrap(),
        signed_in_session(),
        Duration::from_millis(200),
        Duration::from_millis(100),
    )
    .unwrap()
    .with_base(format!("http://{}/youtubei/v1", mock.addr))
    .with_heal_callback(std::sync::Arc::new(|_session: SharedSession| {
        // The healer runs but produces no fresh cookie.
        Box::pin(async {})
    }))
    .await;
    let err = client
        .post("browse", "WEB", json!({"context": {}}))
        .await
        .expect_err("no fresh cookie");
    assert!(matches!(err, innertube::Error::SessionExpired));
}

#[tokio::test]
async fn without_healing_skips_the_callback_and_expires() {
    use std::sync::atomic::{AtomicBool, Ordering as O};
    let mock = Mock::start(vec![Behavior::Respond {
        status: 401,
        headers: vec![],
        body: r#"{"error":{"code":401}}"#.to_string(),
    }])
    .await;
    let called = Arc::new(AtomicBool::new(false));
    let called2 = called.clone();
    let client = Innertube::with_timeouts(
        Clients::embedded().unwrap(),
        signed_in_session(),
        Duration::from_millis(200),
        Duration::from_secs(2),
    )
    .unwrap()
    .with_base(format!("http://{}/youtubei/v1", mock.addr))
    .with_heal_callback(std::sync::Arc::new(move |_session: SharedSession| {
        called2.store(true, O::SeqCst);
        Box::pin(async {})
    }))
    .await;
    let err = innertube::without_healing(client.post("browse", "WEB", json!({"context": {}})))
        .await
        .expect_err("healing disabled");
    assert!(matches!(err, innertube::Error::SessionExpired));
    assert!(!called.load(O::SeqCst), "callback must not run");
}

#[tokio::test]
async fn signed_out_401_is_not_a_session_expiry() {
    let mock = Mock::start(vec![Behavior::Respond {
        status: 401,
        headers: vec![],
        body: r#"{"error":{"code":401}}"#.to_string(),
    }])
    .await;
    let client = Innertube::with_timeouts(
        Clients::embedded().unwrap(),
        SharedSession::default(),
        Duration::from_millis(200),
        Duration::from_secs(2),
    )
    .unwrap()
    .with_base(format!("http://{}/youtubei/v1", mock.addr));
    let err = client
        .post("browse", "WEB", json!({"context": {}}))
        .await
        .expect_err("signed-out 401");
    assert!(matches!(err, innertube::Error::HttpStatus(401)));
}
