//! Scripted PlayerService test — Phase 2/4 integration, headless.
//!
//! Runs ONLY with `--features mpv` (needs system libmpv). Drives the
//! service — not the raw backend — against the generated fixture
//! `tests/fixtures/test-60s.mp4`, `vo=null` / `ao=null`.
//!
//! Covers: load resets position, play advances, pause freezes, resume
//! continues, seek clamps and lands, 2x speed doubles the advance rate,
//! audio-only toggle keeps position, and queue advance hands the next
//! item to the caller for re-resolution. Tolerances mirror
//! mpv_playback.rs (±1.5 s on seeks, ±0.6 s on advance rates).

#![cfg(feature = "mpv")]

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use player::{MpvConfig, PlayerService, QueueItem};
use yt_model::VideoId;

fn fixture_url() -> String {
    let p: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-60s.mp4");
    format!("file://{}", p.display())
}

fn wait_for_position(svc: &mut PlayerService, timeout: Duration) -> f64 {
    let start = Instant::now();
    loop {
        svc.sync_position();
        if svc.position() > 0.0 {
            return svc.position();
        }
        assert!(
            start.elapsed() < timeout,
            "service never reported a position within {timeout:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_seek(svc: &mut PlayerService, target: f64, tol: f64, timeout: Duration) {
    let start = Instant::now();
    loop {
        svc.sync_position();
        if (svc.position() - target).abs() <= tol {
            return;
        }
        assert!(
            start.elapsed() < timeout,
            "seek to {target}s did not settle within {tol}s (last pos: {})",
            svc.position()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn service_drives_playback_end_to_end() {
    let mut svc = PlayerService::new(&MpvConfig::video("/tmp/tubelite-cache".to_string()))
        .expect("mpv must construct");
    svc.set_option("vo", "null").unwrap();
    svc.set_option("ao", "null").unwrap();

    svc.load_pair(&fixture_url(), None).unwrap();
    svc.play().unwrap();
    let pos = wait_for_position(&mut svc, Duration::from_secs(10));
    assert!(pos > 0.0 && pos < 5.0, "position after load+play: {pos}");

    // Pause freezes the clock.
    svc.pause().unwrap();
    let frozen = svc.position();
    thread::sleep(Duration::from_millis(700));
    svc.sync_position();
    assert!(
        (svc.position() - frozen).abs() < 0.4,
        "paused position moved: {frozen} -> {}",
        svc.position()
    );

    // Resume continues.
    svc.play().unwrap();
    thread::sleep(Duration::from_secs(1));
    svc.sync_position();
    assert!(svc.position() > frozen + 0.5, "resume did not advance");

    // Seek lands, clamped to the 60 s fixture.
    svc.seek_to(30.0).unwrap();
    wait_for_seek(&mut svc, 30.0, 1.5, Duration::from_secs(10));
    svc.seek_to(999.0).unwrap();
    // Clamped to the fixture's real duration (60 s nominal; mpv reports
    // ~60.02 s for the generated file, so allow that slop).
    assert!(
        (59.9..=60.1).contains(&svc.state.position_secs),
        "seek past end not clamped: {}",
        svc.state.position_secs
    );

    // 2x speed doubles the advance rate.
    svc.seek_to(10.0).unwrap();
    wait_for_seek(&mut svc, 10.0, 1.5, Duration::from_secs(10));
    svc.set_speed(2.0).unwrap();
    assert_eq!(svc.state.speed, 2.0);
    let before = svc.position();
    thread::sleep(Duration::from_secs(1));
    svc.sync_position();
    let advanced = svc.position() - before;
    assert!(
        (1.4..=2.6).contains(&advanced),
        "2x advance was {advanced}s in 1 s wall"
    );
    svc.set_speed(1.0).unwrap();

    // Audio-only toggle keeps the position.
    let at = svc.set_audio_only(true).unwrap();
    assert!(
        (at - svc.position()).abs() < 1.0,
        "audio-only moved position"
    );
    assert!(svc.state.audio_only);
    svc.set_audio_only(false).unwrap();
    assert!(!svc.state.audio_only);
}

#[test]
fn service_loads_and_clears_subtitles() {
    let mut svc = PlayerService::new(&MpvConfig::video("/tmp/tubelite-cache".to_string()))
        .expect("mpv must construct");
    svc.set_option("vo", "null").unwrap();
    svc.set_option("ao", "null").unwrap();
    svc.load_pair(&fixture_url(), None).unwrap();
    // sub-add needs a loaded file (loadfile is async) — wait for the
    // clock first, mirroring real use where captions load after playback.
    svc.play().unwrap();
    wait_for_position(&mut svc, Duration::from_secs(10));
    let vtt = "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nHello world\n";
    let path = std::env::temp_dir().join("tubelite-test-captions.vtt");
    std::fs::write(&path, vtt).unwrap();
    svc.load_subtitles_file(path.to_str().unwrap(), Some("en".into()))
        .unwrap();
    assert_eq!(svc.state.subtitle_lang.as_deref(), Some("en"));
    svc.disable_subtitles().unwrap();
    assert_eq!(svc.state.subtitle_lang, None);
}

#[test]
fn service_volume_and_mute() {
    let mut svc = PlayerService::new(&MpvConfig::video("/tmp/tubelite-cache".to_string()))
        .expect("mpv must construct");
    svc.set_option("vo", "null").unwrap();
    svc.set_option("ao", "null").unwrap();
    svc.set_volume(42.0).unwrap();
    assert_eq!(svc.state.volume, 42.0);
    svc.set_volume(150.0).unwrap();
    assert_eq!(svc.state.volume, 100.0); // clamped
    svc.set_muted(true).unwrap();
    assert!(svc.state.muted);
    svc.set_muted(false).unwrap();
    assert!(!svc.state.muted);
}

#[test]
fn service_queue_advance() {
    let mut svc = PlayerService::new(&MpvConfig::video("/tmp/tubelite-cache".to_string()))
        .expect("mpv must construct");
    svc.set_option("vo", "null").unwrap();
    svc.set_option("ao", "null").unwrap();
    svc.queue = player::Queue::new(vec![
        QueueItem {
            video_id: VideoId("aaaaaaaaaaa".into()),
            title: "first".into(),
            resume_secs: None,
        },
        QueueItem {
            video_id: VideoId("bbbbbbbbbbb".into()),
            title: "second".into(),
            resume_secs: None,
        },
    ]);
    assert_eq!(svc.queue.current().unwrap().title, "first");
    let next = svc.advance_queue().unwrap();
    assert_eq!(next.title, "second");
    assert!(
        svc.advance_queue().is_none(),
        "queue should end after the last item"
    );
}
