//! Autoplay + prefetch with a fake Resolver and a fake mpv control.
//! No yt-dlp, no libmpv, no network.
//!
//! The fakes mirror `tests/player_recovery.rs`: the player owns a
//! `Box<dyn MpvControl>` over an `Arc<FakeMpv>`, and the test keeps
//! the other `Arc` to script events and inspect calls. The resolver
//! here counts calls per video id, so the tests can prove the next
//! item was resolved *before* the current one ended and was *not*
//! re-resolved on the advance.

use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use player::{
    MpvControl, MpvEvent, Player, PlayerEvent, PlayerState, QueueItem, RepeatMode, ResolveError,
    Resolved, Resolver,
};
use tokio::sync::broadcast;
use yt_model::VideoId;

struct FakeResolver {
    calls: Mutex<Vec<(String, u32)>>,
    /// Ids whose *first* resolve fails (to simulate a failed
    /// prefetch); later resolves succeed.
    fail_once: Mutex<HashSet<String>>,
    /// Ids whose *every* resolve fails (to simulate a video that
    /// cannot be resolved at all, e.g. on an autoplay advance).
    fail_always: Mutex<HashSet<String>>,
}

impl FakeResolver {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail_once: Mutex::new(HashSet::new()),
            fail_always: Mutex::new(HashSet::new()),
        }
    }

    fn calls(&self) -> Vec<(String, u32)> {
        self.calls.lock().unwrap().clone()
    }

    fn calls_for(&self, video_id: &str) -> usize {
        self.calls().iter().filter(|(id, _)| id == video_id).count()
    }

    fn fail_first_resolve_of(&self, video_id: &str) {
        self.fail_once.lock().unwrap().insert(video_id.to_string());
    }

    fn fail_every_resolve_of(&self, video_id: &str) {
        self.fail_always
            .lock()
            .unwrap()
            .insert(video_id.to_string());
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
            if self.fail_always.lock().unwrap().contains(&video_id) {
                return Err(ResolveError::YtDlpFailed {
                    video_id: video_id.clone(),
                    stderr: "fake permanent failure".into(),
                });
            }
            if self.fail_once.lock().unwrap().remove(&video_id) {
                return Err(ResolveError::YtDlpFailed {
                    video_id: video_id.clone(),
                    stderr: "fake first-attempt failure".into(),
                });
            }
            Ok(Resolved {
                video_url: format!("https://example.com/{video_id}-{max_height}.mp4"),
                audio_url: Some("https://example.com/a.m4a".into()),
                headers: vec![],
                expires_at: None,
                title: format!("Fake {video_id}"),
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

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn loaded_ids(&self) -> Vec<String> {
        // load_pair URLs carry the video id: .../<id>-<height>.mp4
        self.calls()
            .iter()
            .filter_map(|c| c.strip_prefix("load_pair https://example.com/"))
            .filter_map(|rest| rest.split('-').next().map(str::to_string))
            .collect()
    }
}

/// Newtype so the fake can implement `MpvControl` (the orphan rule
/// forbids `impl MpvControl for Arc<FakeMpv>` directly).
struct SharedMpv(Arc<FakeMpv>);

impl MpvControl for SharedMpv {
    fn load_pair(
        &mut self,
        video_url: &str,
        audio_url: Option<&str>,
    ) -> Result<(), player::PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("load_pair {video_url} audio={audio_url:?}"));
        Ok(())
    }

    fn set_pause(&mut self, pause: bool) -> Result<(), player::PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("set_pause {pause}"));
        Ok(())
    }

    fn seek(&mut self, secs: f64) -> Result<(), player::PlayerError> {
        *self.0.position.lock().unwrap() = secs;
        self.0.calls.lock().unwrap().push(format!("seek {secs}"));
        Ok(())
    }

    fn set_option(&mut self, key: &str, value: &str) -> Result<(), player::PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("set_option {key}={value}"));
        Ok(())
    }

    fn set_window(&mut self, _wid: i64) -> Result<(), player::PlayerError> {
        Ok(())
    }

    fn load_subtitles(&mut self, path: &str) -> Result<(), player::PlayerError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(format!("load_subtitles {path}"));
        Ok(())
    }

    fn disable_subtitles(&mut self) -> Result<(), player::PlayerError> {
        Ok(())
    }

    fn apply_stage1_options(&mut self, _cache_dir: &Path) -> Result<(), player::PlayerError> {
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

fn item(id: &str) -> QueueItem {
    QueueItem {
        video_id: VideoId(id.to_string()),
        title: format!("Video {id}"),
        resume_secs: None,
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

fn spawn_pump(player: &Player) -> tokio::task::JoinHandle<()> {
    tokio::spawn({
        let player = player.clone();
        async move { player.run_event_loop().await }
    })
}

#[tokio::test]
async fn prefetch_resolves_next_before_end_and_advance_uses_cache() {
    let (player, resolver, mpv) = make_player();
    let mut rx = player.subscribe();
    let pump = spawn_pump(&player);
    player
        .set_queue(vec![item("v0"), item("v1"), item("v2")], Some("v0"))
        .await;

    player.play("v0").await.unwrap();
    assert_eq!(resolver.calls_for("v0"), 1);

    // The next item is resolved while v0 is still playing — before
    // any end event — and exactly once.
    wait_for("prefetch of v1", || resolver.calls_for("v1") == 1).await;

    // Natural end: autoplay advances to v1 without a second resolve.
    mpv.push_event(MpvEvent::EndFileEof);
    wait_for("v1 loaded", || mpv.loaded_ids().contains(&"v1".to_string())).await;
    assert_eq!(
        resolver.calls_for("v1"),
        1,
        "advance must use the prefetched resolve, not re-resolve"
    );
    assert_eq!(player.current_video_id().await.as_deref(), Some("v1"));
    assert_eq!(player.state().await, PlayerState::Playing);

    // While v1 plays, v2 is prefetched; the next advance is cached too.
    wait_for("prefetch of v2", || resolver.calls_for("v2") == 1).await;
    mpv.push_event(MpvEvent::EndFileEof);
    wait_for("v2 loaded", || mpv.loaded_ids().contains(&"v2".to_string())).await;
    assert_eq!(resolver.calls_for("v2"), 1);

    // Queue exhausted (repeat Off): the player reports Ended.
    mpv.push_event(MpvEvent::EndFileEof);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout(
            deadline.saturating_duration_since(tokio::time::Instant::now()),
            rx.recv(),
        )
        .await
        {
            Ok(Ok(PlayerEvent::Ended)) => break,
            Ok(Ok(_)) => continue,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(e)) => panic!("broadcast recv failed: {e}"),
            Err(_) => panic!("timed out waiting for Ended"),
        }
    }
    assert_eq!(player.state().await, PlayerState::Ended);
    assert_eq!(mpv.loaded_ids(), vec!["v0", "v1", "v2"]);

    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn failed_prefetch_falls_back_to_resolve_on_advance() {
    let (player, resolver, mpv) = make_player();
    let pump = spawn_pump(&player);
    resolver.fail_first_resolve_of("v1");
    player
        .set_queue(vec![item("v0"), item("v1")], Some("v0"))
        .await;

    player.play("v0").await.unwrap();
    // The prefetch of v1 fails once (and is silent)...
    wait_for("failed prefetch of v1", || resolver.calls_for("v1") == 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(player.state().await, PlayerState::Playing);

    // ...but the advance still plays v1 by resolving it again.
    mpv.push_event(MpvEvent::EndFileEof);
    wait_for("v1 loaded", || mpv.loaded_ids().contains(&"v1".to_string())).await;
    assert_eq!(resolver.calls_for("v1"), 2);
    assert_eq!(player.current_video_id().await.as_deref(), Some("v1"));

    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn failed_autoplay_advance_leaves_player_in_error_state() {
    // The reported bug: when the next video cannot be resolved, the
    // Watch page froze silently at the end of the finished video.
    // The player must land in Error with the message preserved (so
    // `player_status` can surface it), not report Ended and not
    // leave the current video pointing at the failed item.
    let (player, resolver, mpv) = make_player();
    let pump = spawn_pump(&player);
    resolver.fail_every_resolve_of("v1");
    player
        .set_queue(vec![item("v0"), item("v1")], Some("v0"))
        .await;

    player.play("v0").await.unwrap();
    assert_eq!(player.state().await, PlayerState::Playing);
    // The prefetch of v1 fails (silently) while v0 still plays.
    wait_for("failed prefetch of v1", || resolver.calls_for("v1") == 1).await;

    // Natural end: the autoplay advance re-resolves v1, fails again.
    mpv.push_event(MpvEvent::EndFileEof);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let state = loop {
        let s = player.state().await;
        if matches!(s, PlayerState::Error(_)) {
            break s;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for the Error state; last: {s:?}");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let PlayerState::Error(message) = state else {
        unreachable!()
    };
    assert!(
        message.contains("resolve failed"),
        "error must name the failure: {message}"
    );
    assert!(
        message.contains("v1"),
        "error must name the video: {message}"
    );
    assert_eq!(
        player.current_video_id().await.as_deref(),
        Some("v0"),
        "the failed advance must not move the current video"
    );
    assert!(
        !mpv.loaded_ids().contains(&"v1".to_string()),
        "v1 was never loaded"
    );

    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn autoplay_respects_repeat_all_and_one() {
    // Repeat All wraps to the first item.
    let (player, resolver, mpv) = make_player();
    let pump = spawn_pump(&player);
    player
        .set_queue(vec![item("v0"), item("v1")], Some("v0"))
        .await;
    player.set_repeat(RepeatMode::All).await;
    player.play("v0").await.unwrap();
    mpv.push_event(MpvEvent::EndFileEof);
    wait_for("v1 loaded", || mpv.loaded_ids().contains(&"v1".to_string())).await;
    mpv.push_event(MpvEvent::EndFileEof);
    wait_for("v0 loaded again", || {
        mpv.loaded_ids().iter().filter(|id| *id == "v0").count() == 2
    })
    .await;
    assert_eq!(player.current_video_id().await.as_deref(), Some("v0"));
    let _ = resolver;
    player.stop();
    pump.await.unwrap();

    // Repeat One replays the same item.
    let (player, _resolver, mpv) = make_player();
    let pump = spawn_pump(&player);
    player
        .set_queue(vec![item("v0"), item("v1")], Some("v0"))
        .await;
    player.set_repeat(RepeatMode::One).await;
    player.play("v0").await.unwrap();
    mpv.push_event(MpvEvent::EndFileEof);
    wait_for("v0 replayed", || {
        mpv.loaded_ids().iter().filter(|id| *id == "v0").count() == 2
    })
    .await;
    assert_eq!(player.current_video_id().await.as_deref(), Some("v0"));
    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn next_advances_and_returns_none_at_end() {
    let (player, _resolver, mpv) = make_player();
    let pump = spawn_pump(&player);
    player
        .set_queue(vec![item("v0"), item("v1"), item("v2")], Some("v0"))
        .await;
    player.play("v0").await.unwrap();

    assert_eq!(player.next().await.unwrap().as_deref(), Some("v1"));
    wait_for("v1 loaded", || mpv.loaded_ids().contains(&"v1".to_string())).await;
    assert_eq!(player.peek_next_id().await.as_deref(), Some("v2"));

    assert_eq!(player.next().await.unwrap().as_deref(), Some("v2"));
    wait_for("v2 loaded", || mpv.loaded_ids().contains(&"v2".to_string())).await;

    // Exhausted (repeat Off): None, and the current video keeps playing.
    assert_eq!(player.next().await.unwrap(), None);
    assert_eq!(player.current_video_id().await.as_deref(), Some("v2"));
    assert_eq!(player.state().await, PlayerState::Playing);

    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn set_queue_while_playing_prefetches_next() {
    // The Watch page sets the queue after playback already started
    // (Up Next loads async): the prefetch must still kick in.
    let (player, resolver, mpv) = make_player();
    let pump = spawn_pump(&player);
    player.play("v0").await.unwrap();
    assert_eq!(resolver.calls_for("v1"), 0);
    player
        .set_queue(vec![item("v0"), item("v1")], Some("v0"))
        .await;
    wait_for("prefetch of v1", || resolver.calls_for("v1") == 1).await;

    mpv.push_event(MpvEvent::EndFileEof);
    wait_for("v1 loaded", || mpv.loaded_ids().contains(&"v1".to_string())).await;
    assert_eq!(resolver.calls_for("v1"), 1);

    let snapshot = player.queue_snapshot().await;
    assert_eq!(snapshot.index, Some(1));
    assert_eq!(snapshot.items.len(), 2);

    player.stop();
    pump.await.unwrap();
}

#[tokio::test]
async fn manual_play_of_queued_item_moves_the_pointer() {
    let (player, _resolver, mpv) = make_player();
    let mut rx = player.subscribe();
    let pump = spawn_pump(&player);
    player
        .set_queue(vec![item("v0"), item("v1"), item("v2")], Some("v0"))
        .await;
    // The user clicks an Up Next item instead of waiting for autoplay.
    player.play("v2").await.unwrap();
    wait_for("v2 loaded", || mpv.loaded_ids().contains(&"v2".to_string())).await;
    assert_eq!(player.queue_snapshot().await.index, Some(2));
    // No next item after v2: a natural end reports Ended.
    mpv.push_event(MpvEvent::EndFileEof);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match tokio::time::timeout(
            deadline.saturating_duration_since(tokio::time::Instant::now()),
            rx.recv(),
        )
        .await
        {
            Ok(Ok(PlayerEvent::Ended)) => break,
            Ok(Ok(_)) => continue,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(e)) => panic!("broadcast recv failed: {e}"),
            Err(_) => panic!("timed out waiting for Ended"),
        }
    }
    assert_eq!(player.state().await, PlayerState::Ended);
    player.stop();
    pump.await.unwrap();
}
