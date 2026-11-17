//! Recovery logic with a fake Resolver and a fake mpv control.
//! No yt-dlp, no libmpv, no network.
//!
//! The fakes are shared with the test through `Arc`: the player owns
//! a `Box<dyn MpvControl>` over an `Arc<FakeMpv>`, and the test keeps
//! the other `Arc` to script events and inspect calls.

use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use player::{
    MpvControl, MpvEvent, Player, PlayerError, PlayerEvent, PlayerState, ResolveError, Resolved,
    Resolver,
};
use tokio::sync::broadcast;

struct FakeResolver {
    calls: Mutex<Vec<(String, u32)>>,
}

impl FakeResolver {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(String, u32)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Resolver for FakeResolver {
    fn resolve(
        &self,
        video_id: &str,
        max_height: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Resolved, ResolveError>> + Send + '_>> {
        let video_id = video_id.to_string();
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((video_id.clone(), max_height));
            Ok(Resolved {
                video_url: format!("https://example.com/v{max_height}.mp4"),
                audio_url: Some("https://example.com/a.m4a".into()),
                headers: vec![("User-Agent".into(), "fake-agent".into())],
                expires_at: None,
                title: "Fake".into(),
                duration: Some(213.0),
                tracking_url: None,
            })
        })
    }
}

struct FakeMpv {
    events: Mutex<VecDeque<MpvEvent>>,
    calls: Mutex<Vec<String>>,
    position: Mutex<f64>,
}

impl FakeMpv {
    fn new() -> Self {
        Self {
            events: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
            position: Mutex::new(0.0),
        }
    }

    fn push_event(&self, event: MpvEvent) {
        self.events.lock().unwrap().push_back(event);
    }

    fn set_position(&self, secs: f64) {
        *self.position.lock().unwrap() = secs;
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

/// Newtype so the fake can implement `MpvControl` (the orphan rule
/// forbids `impl MpvControl for Arc<FakeMpv>` directly).
struct SharedMpv(Arc<FakeMpv>);

impl MpvControl for SharedMpv {
    fn load_pair(&mut self, video_url: &str, audio_url: Option<&str>) -> Result<(), PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("load_pair {video_url} audio={audio_url:?}"));
        Ok(())
    }

    fn set_pause(&mut self, pause: bool) -> Result<(), PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("set_pause {pause}"));
        Ok(())
    }

    fn seek(&mut self, secs: f64) -> Result<(), PlayerError> {
        *self.0.position.lock().unwrap() = secs;
        self.0.calls.lock().unwrap().push(format!("seek {secs}"));
        Ok(())
    }

    fn set_option(&mut self, key: &str, value: &str) -> Result<(), PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("set_option {key}={value}"));
        Ok(())
    }

    fn load_subtitles(&mut self, path: &str) -> Result<(), PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("load_subtitles {path}"));
        Ok(())
    }

    fn disable_subtitles(&mut self) -> Result<(), PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push("disable_subtitles".to_string());
        Ok(())
    }

    fn apply_stage1_options(&mut self, cache_dir: &Path) -> Result<(), PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("stage1 {}", cache_dir.display()));
        Ok(())
    }

    fn time_pos(&self) -> Option<f64> {
        Some(*self.0.position.lock().unwrap())
    }

    fn duration(&self) -> Option<f64> {
        Some(213.0)
    }

    fn wait_event(&mut self, _timeout: Duration) -> Option<MpvEvent> {
        self.0.events.lock().unwrap().pop_front()
    }
}

fn make_player() -> (Player, Arc<FakeResolver>, Arc<FakeMpv>) {
    let resolver = Arc::new(FakeResolver::new());
    let mpv = Arc::new(FakeMpv::new());
    let player = Player::new(
        Arc::clone(&resolver) as Arc<dyn Resolver>,
        Box::new(SharedMpv(Arc::clone(&mpv))),
        PathBuf::from("/tmp/player-test-cache"),
        720,
    )
    .expect("player construction");
    (player, resolver, mpv)
}

/// Wait until `cond` is true, or panic with `what` after the timeout.
async fn wait_for<F: Fn() -> bool>(what: &str, cond: F) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// Drain broadcast events until one matches `want`, or time out.
async fn wait_for_event(
    rx: &mut broadcast::Receiver<PlayerEvent>,
    want: &PlayerEvent,
) -> PlayerEvent {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for event {want:?}");
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(event)) if &event == want => return event,
            Ok(Ok(_)) => continue,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(e)) => panic!("broadcast recv failed: {e}"),
            Err(_) => panic!("timed out waiting for event {want:?}"),
        }
    }
}

#[tokio::test]
async fn end_file_error_recovers_once_then_fails() {
    let (player, resolver, mpv) = make_player();
    let mut rx = player.subscribe();
    let pump = tokio::spawn({
        let player = player.clone();
        async move { player.run_event_loop().await }
    });

    player.play("dQw4w9WgXcQ").await.unwrap();
    assert_eq!(resolver.calls().len(), 1);
    assert!(mpv
        .calls()
        .iter()
        .any(|c| c.contains("load_pair https://example.com/v720.mp4")));
    // Stage-1 options were applied at construction, headers at load.
    assert!(mpv.calls().iter().any(|c| c.starts_with("stage1 ")));
    assert!(mpv
        .calls()
        .iter()
        .any(|c| c.contains("http-header-fields=User-Agent: fake-agent")));

    // Let playback advance to 42 s, then kill the stream.
    mpv.set_position(42.0);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if player.position().await >= 42.0 {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for position 42");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    mpv.push_event(MpvEvent::EndFileError);
    wait_for_event(&mut rx, &PlayerEvent::Recovering { attempt: 1 }).await;
    wait_for_event(&mut rx, &PlayerEvent::Recovered).await;
    wait_for("second resolve", || resolver.calls().len() == 2).await;
    assert!(mpv.calls().iter().any(|c| c == "seek 42"));

    // A second failure for the same item is terminal: no third
    // resolve, state Error.
    mpv.push_event(MpvEvent::EndFileError);
    wait_for_event(
        &mut rx,
        &PlayerEvent::Error("stream failed after re-resolve".to_string()),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(resolver.calls().len(), 2);
    assert_eq!(
        player.state().await,
        PlayerState::Error("stream failed after re-resolve".to_string())
    );

    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn log_403_triggers_the_same_recovery() {
    let (player, resolver, mpv) = make_player();
    let mut rx = player.subscribe();
    let pump = tokio::spawn({
        let player = player.clone();
        async move { player.run_event_loop().await }
    });

    player.play("dQw4w9WgXcQ").await.unwrap();
    mpv.set_position(10.0);
    tokio::time::sleep(Duration::from_millis(400)).await;

    mpv.push_event(MpvEvent::Log403);
    wait_for_event(&mut rx, &PlayerEvent::Recovering { attempt: 1 }).await;
    wait_for_event(&mut rx, &PlayerEvent::Recovered).await;
    assert_eq!(resolver.calls().len(), 2);

    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn natural_end_does_not_recover() {
    let (player, resolver, mpv) = make_player();
    let mut rx = player.subscribe();
    let pump = tokio::spawn({
        let player = player.clone();
        async move { player.run_event_loop().await }
    });

    player.play("dQw4w9WgXcQ").await.unwrap();
    mpv.push_event(MpvEvent::EndFileEof);
    wait_for_event(&mut rx, &PlayerEvent::Ended).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(resolver.calls().len(), 1);
    assert_eq!(player.state().await, PlayerState::Ended);

    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn set_quality_re_resolves_and_resumes_at_position() {
    let (player, resolver, mpv) = make_player();
    let pump = tokio::spawn({
        let player = player.clone();
        async move { player.run_event_loop().await }
    });

    player.play("dQw4w9WgXcQ").await.unwrap();
    mpv.set_position(30.0);
    tokio::time::sleep(Duration::from_millis(400)).await;
    player.set_quality(1080).await.unwrap();

    wait_for("resolve at 1080", || {
        resolver.calls().iter().any(|(_, h)| *h == 1080)
    })
    .await;
    assert!(mpv
        .calls()
        .iter()
        .any(|c| c.contains("load_pair https://example.com/v1080.mp4")));
    assert!(mpv.calls().iter().any(|c| c == "seek 30"));
    assert_eq!(player.state().await, PlayerState::Playing);

    player.stop();
    pump.await.unwrap();
}
