//! A rotated `Set-Cookie` ends up persisted.
//!
//! A mock InnerTube server answers one browse with `Set-Cookie`
//! headers that rotate the SAPISID. The Innertube client merges them
//! into the live session (`merge_set_cookie` → `cookie_changed`),
//! and the manager's background sync task writes the merged cookie
//! back to the secret store for the active account. No real network,
//! no keyring: the secret store is in-memory here.
//!
//! Cookie values are compared, never printed.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use accounts::{
    AccountStore, CookieSet, Error, LoginFlow, MemorySecretStore, SecretStore, SessionManager,
};
use innertube::{Clients, Innertube, Session, SharedSession};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A flow that never gets called in this test.
struct NeverFlow;

impl LoginFlow for NeverFlow {
    fn interactive_login(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>> {
        Box::pin(async move { Err(Error::Flow("unused".into())) })
    }

    fn silent_refresh(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<CookieSet, Error>> + Send + '_>> {
        Box::pin(async move { Err(Error::Flow("unused".into())) })
    }
}

/// Serve one request with a 200 + two `Set-Cookie` headers, then stop.
async fn mock_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        // Read the request head + body (Content-Length).
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let mut content_length = 0usize;
        loop {
            let Ok(n) = stream.read(&mut chunk).await else {
                return;
            };
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                for line in head.lines().skip(1) {
                    if let Some((name, value)) = line.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("content-length") {
                            content_length = value.trim().parse().unwrap_or(0);
                        }
                    }
                }
                if buf.len() >= pos + 4 + content_length {
                    break;
                }
            }
        }
        let body = "{}";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
             Set-Cookie: SAPISID=rotated; Path=/; Domain=.youtube.com\r\n\
             Set-Cookie: VISITOR_INFO1_LIVE=v2; Path=/; Domain=.youtube.com\r\n\
             Connection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });
    addr
}

#[tokio::test]
async fn rotated_set_cookie_ends_up_persisted() {
    let store = Arc::new(Mutex::new(AccountStore::open_in_memory().unwrap()));
    let secrets = Arc::new(MemorySecretStore::new());
    let manager = Arc::new(SessionManager::new(
        Arc::clone(&store),
        Arc::clone(&secrets) as Arc<dyn accounts::SecretStore>,
        SharedSession::new(Session::default()),
        Arc::new(NeverFlow),
    ));
    // Seed the session with the old cookie.
    manager
        .add_account("SAPISID=old; VISITOR_INFO1_LIVE=v1")
        .await
        .unwrap();
    let account_id = manager.status().await.account_id.unwrap();

    // The sync task must be waiting on `cookie_changed` before the
    // rotation arrives, or the notification is missed (the 60 s tick
    // would recover it, but not inside this test's deadline).
    let _sync = manager.spawn_cookie_sync();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // One browse against the mock: the client merges the Set-Cookie
    // headers into the live session.
    let addr = mock_server().await;
    let client = Innertube::new(Clients::embedded().unwrap(), manager.session().clone())
        .unwrap()
        .with_base(format!("http://{addr}"));
    let body = client
        .post(
            "browse",
            "WEB",
            json!({"context": {}, "browseId": "FEwhat_to_watch"}),
        )
        .await
        .unwrap();
    assert!(body.is_object());

    // The merged cookie must reach the secret store for the active
    // account: the SAPISID rotated, the visitor cookie rotated too,
    // and the old SAPISID is gone.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let stored = secrets.get(&account_id).unwrap().unwrap();
        let value = stored.expose();
        if value.contains("SAPISID=rotated") {
            assert!(value.contains("VISITOR_INFO1_LIVE=v2"));
            assert!(!value.contains("SAPISID=old"));
            assert!(!value.contains("VISITOR_INFO1_LIVE=v1"));
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("rotated cookie was not persisted within 5 s");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
