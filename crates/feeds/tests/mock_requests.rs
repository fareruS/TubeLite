//! The feed functions against a local mock server: assert each one
//! sends the endpoint and body shape from its fixture, and that
//! `home_continuation` and `watch_next` parse real fixture bodies
//! end to end. No network beyond localhost.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use feeds::Feeds;
use innertube::{Clients, Innertube, SharedSession};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct Mock {
    addr: SocketAddr,
    last_request: Arc<Mutex<Option<(String, String)>>>,
}

impl Mock {
    async fn start(response_body: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let last_request = Arc::new(Mutex::new(None));
        let recorded = last_request.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let recorded = recorded.clone();
                let body = response_body.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let mut content_length = 0usize;
                    let mut head_end = None;
                    while head_end.is_none() {
                        let Ok(n) = stream.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            head_end = Some(pos);
                            let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                            for line in head.lines().skip(1) {
                                if let Some((name, value)) = line.split_once(':') {
                                    if name.trim().eq_ignore_ascii_case("content-length") {
                                        content_length = value.trim().parse().unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                    let head_end = head_end.unwrap();
                    while buf.len() < head_end + 4 + content_length {
                        let Ok(n) = stream.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let path = head
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("")
                        .to_string();
                    let body_text =
                        String::from_utf8_lossy(&buf[head_end + 4..head_end + 4 + content_length])
                            .to_string();
                    *recorded.lock().unwrap() = Some((path, body_text));
                    let out = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(out.as_bytes()).await;
                });
            }
        });
        Mock { addr, last_request }
    }

    fn request(&self) -> (String, Value) {
        let (path, body) = self
            .last_request
            .lock()
            .unwrap()
            .clone()
            .expect("mock received a request");
        (
            path,
            serde_json::from_str(&body).expect("request body is json"),
        )
    }
}

fn feeds_for(mock: &Mock) -> Feeds {
    let inner = Innertube::with_timeouts(
        Clients::embedded().unwrap(),
        SharedSession::default(),
        Duration::from_millis(500),
        Duration::from_secs(2),
    )
    .unwrap()
    .with_base(format!("http://{}/youtubei/v1", mock.addr));
    Feeds::new(Arc::new(inner))
}

fn fixture_text(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/redacted")
        .join(name);
    std::fs::read_to_string(&path).expect("read fixture")
}

#[tokio::test]
async fn home_posts_the_fixture_browse_shape() {
    let mock = Mock::start("{}".to_string()).await;
    let feeds = feeds_for(&mock);
    let _ = feeds.home().await;
    let (path, body) = mock.request();
    assert!(path.starts_with("/youtubei/v1/browse?prettyPrint=false"));
    assert_eq!(body["browseId"], "FEwhat_to_watch");
    assert_eq!(body["inlineSettingStatus"], "INLINE_SETTING_STATUS_ON");
    assert_eq!(body["context"]["client"]["clientName"], "WEB");
    assert_eq!(
        body["context"]["client"]["clientVersion"],
        "2.20261112.10.00"
    );
}

#[tokio::test]
async fn subscriptions_and_history_post_their_browse_ids() {
    let mock = Mock::start("{}".to_string()).await;
    let feeds = feeds_for(&mock);
    let _ = feeds.subscriptions().await;
    assert_eq!(mock.request().1["browseId"], "FEsubscriptions");
    let _ = feeds.history().await;
    assert_eq!(mock.request().1["browseId"], "FEhistory");
}

#[tokio::test]
async fn home_continuation_posts_a_bare_token_and_parses_the_fixture() {
    let mock = Mock::start(fixture_text("continuation.response.json")).await;
    let feeds = feeds_for(&mock);
    let page = feeds
        .home_continuation("4qmFsgLWEBIPtest")
        .await
        .expect("continuation");
    let (path, body) = mock.request();
    assert!(path.starts_with("/youtubei/v1/browse?prettyPrint=false"));
    assert_eq!(body["continuation"], "4qmFsgLWEBIPtest");
    assert!(body.get("browseId").is_none());
    assert_eq!(page.items.len(), 20);
    assert_eq!(page.continuation.as_ref().map(String::len), Some(4028));
}

#[tokio::test]
async fn search_posts_query_and_optional_filter_params() {
    let mock = Mock::start("{}".to_string()).await;
    let feeds = feeds_for(&mock);
    let _ = feeds.search("rust programming", None).await;
    let (path, body) = mock.request();
    assert!(path.starts_with("/youtubei/v1/search?prettyPrint=false"));
    assert_eq!(body["query"], "rust programming");
    assert_eq!(body["inlineSettingStatus"], "INLINE_SETTING_STATUS_ON");
    assert!(body.get("params").is_none());
    let _ = feeds.search("rust", Some("EgQQARgB")).await;
    assert_eq!(mock.request().1["params"], "EgQQARgB");
}

#[tokio::test]
async fn channel_videos_posts_the_videos_tab_params() {
    let mock = Mock::start("{}".to_string()).await;
    let feeds = feeds_for(&mock);
    let _ = feeds.channel_videos("UCt3JiNkefsfbA2N4SgEkoiQ").await;
    let body = mock.request().1;
    assert_eq!(body["browseId"], "UCt3JiNkefsfbA2N4SgEkoiQ");
    assert_eq!(body["params"], "EgZ2aWRlb3PyBgQKAjoA");
}

#[tokio::test]
async fn watch_next_posts_get_watch_and_parses_up_next() {
    let mock = Mock::start(fixture_text("watch-next.response.json")).await;
    let feeds = feeds_for(&mock);
    let page = feeds.watch_next("7wuaLanSqDs").await.expect("watch-next");
    let (path, body) = mock.request();
    assert!(path.starts_with("/youtubei/v1/get_watch?prettyPrint=false"));
    assert_eq!(body["playerRequest"]["videoId"], "7wuaLanSqDs");
    assert_eq!(body["watchNextRequest"]["videoId"], "7wuaLanSqDs");
    assert_eq!(page.items.len(), 20);
}

#[tokio::test]
async fn playlist_library_and_watch_later_post_their_guide_browse_ids() {
    let mock = Mock::start("{}".to_string()).await;
    let feeds = feeds_for(&mock);
    let _ = feeds.playlist("VLPLtest").await;
    assert_eq!(mock.request().1["browseId"], "VLPLtest");
    let _ = feeds.library_playlists().await;
    assert_eq!(mock.request().1["browseId"], "FEplaylist_aggregation");
    let _ = feeds.watch_later().await;
    assert_eq!(mock.request().1["browseId"], "VLWL");
}

#[tokio::test]
async fn search_suggestions_names_the_capture_needed() {
    let mock = Mock::start("{}".to_string()).await;
    let feeds = feeds_for(&mock);
    let err = feeds
        .search_suggestions("rust")
        .await
        .expect_err("no suggestions endpoint exists");
    let message = err.to_string();
    assert!(message.contains("searchbox") || message.contains("get_search_suggestions"));
    // No request was ever sent.
    assert!(mock.last_request.lock().unwrap().is_none());
}
