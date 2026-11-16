//! Scripted playback test — Phase 2 acceptance, headless.
//!
//! Runs ONLY with `--features mpv` (needs system libmpv). Uses the
//! generated fixture `tests/fixtures/test-60s.mp4` (60 s, 320x180 testsrc
//! + 440 Hz sine, ffmpeg-generated) — no network, no YouTube, no display:
//!   `vo=null` / `ao=null`, so no GL context or audio device is needed.
//!
//! Covers: play advances position, pause freezes it, resume continues,
//! 20 deterministic random seeks land within tolerance, speed 2x doubles
//! the advance rate, audio-only toggle works, and `avsync` stays bounded
//! after the seeks (headless A/V sync proxy — the real render-window
//! A/V check still needs Phase 0's Windows spike / a GL render test).
//!
//! Timing note: mpv's `time-pos` is polled, not event-driven, so
//! tolerances are generous (±1.5 s on seeks, ±0.6 s on advance rates).
//! A failure here means the backend plumbing is broken, not that a
//! single frame was late.

#![cfg(feature = "mpv")]

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use player::{MpvConfig, MpvPlayer};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-60s.mp4")
}

/// Wait until `time_pos` reports a value, or fail with a timeout.
fn wait_for_position(player: &MpvPlayer, timeout: Duration) -> f64 {
    let start = Instant::now();
    loop {
        if let Some(pos) = player.time_pos() {
            return pos;
        }
        assert!(
            start.elapsed() < timeout,
            "mpv never reported time-pos within {timeout:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// Wait until `time_pos` is within `tol` of `target` (or fail).
fn wait_for_seek(player: &MpvPlayer, target: f64, tol: f64, timeout: Duration) {
    let start = Instant::now();
    loop {
        if let Some(pos) = player.time_pos() {
            if (pos - target).abs() <= tol {
                return;
            }
        }
        assert!(
            start.elapsed() < timeout,
            "seek to {target}s did not settle within {tol}s after {timeout:?} (last pos: {:?})",
            player.time_pos()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn scripted_playback_play_pause_seek_speed() {
    let fixture = fixture_path();
    assert!(fixture.exists(), "missing fixture: {}", fixture.display());

    let cache_dir = std::env::temp_dir().join("tubelite-test-cache");
    let _ = std::fs::create_dir_all(&cache_dir);
    let player =
        MpvPlayer::new(&MpvConfig::video(cache_dir.to_string_lossy())).expect("MpvPlayer::new");
    // Headless: no GL window, no audio device.
    player.set_option("vo", "null").unwrap();
    player.set_option("ao", "null").unwrap();

    player
        .load_pair(fixture.to_string_lossy().as_ref(), None)
        .expect("loadfile");

    // Duration is reported once the file is probed.
    let start = Instant::now();
    let duration = loop {
        if let Some(d) = player.duration() {
            break d;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(50));
    };
    assert!((duration - 60.0).abs() < 1.0, "duration was {duration}");

    // 1. Play advances position.
    let p0 = wait_for_position(&player, Duration::from_secs(10));
    thread::sleep(Duration::from_secs(2));
    let p1 = wait_for_position(&player, Duration::from_secs(5));
    assert!(
        p1 - p0 > 1.0 && p1 - p0 < 3.0,
        "position did not advance ~2s in 2s of wall time: {p0} -> {p1}"
    );

    // 2. Pause freezes position.
    player.set_pause(true).unwrap();
    thread::sleep(Duration::from_millis(300));
    let paused_a = wait_for_position(&player, Duration::from_secs(5));
    thread::sleep(Duration::from_secs(1));
    let paused_b = wait_for_position(&player, Duration::from_secs(5));
    assert!(
        (paused_b - paused_a).abs() < 0.3,
        "position moved while paused: {paused_a} -> {paused_b}"
    );

    // 3. Resume continues from the same position.
    player.set_pause(false).unwrap();
    thread::sleep(Duration::from_secs(1));
    let resumed = wait_for_position(&player, Duration::from_secs(5));
    assert!(
        resumed > paused_b && resumed - paused_b < 2.0,
        "resume did not continue from pause point: {paused_b} -> {resumed}"
    );

    // 4. 20 deterministic pseudo-random seeks (LCG, seed 42), each must
    //    settle within ±1.5s, and avsync must stay bounded afterwards.
    let mut seed: u64 = 42;
    let mut max_avsync: f64 = 0.0;
    for i in 0..20 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let target = 2.0 + (seed % 54_000) as f64 / 1000.0; // 2.0 .. 56.0
        player.seek(target).expect("seek");
        wait_for_seek(&player, target, 1.5, Duration::from_secs(10));
        if let Some(a) = player.avsync() {
            max_avsync = max_avsync.max(a.abs());
        }
        let _ = i;
    }
    assert!(
        max_avsync < 0.5,
        "avsync drifted after 20 seeks: {max_avsync}s (limit 0.5s)"
    );

    // 5. Speed 2x doubles the advance rate.
    player.seek(5.0).unwrap();
    wait_for_seek(&player, 5.0, 1.5, Duration::from_secs(10));
    player.set_speed(2.0).unwrap();
    let s0 = wait_for_position(&player, Duration::from_secs(5));
    thread::sleep(Duration::from_secs(2));
    let s1 = wait_for_position(&player, Duration::from_secs(5));
    assert!(
        s1 - s0 > 3.0 && s1 - s0 < 5.5,
        "2x speed did not advance ~4s in 2s of wall time: {s0} -> {s1}"
    );
    player.set_speed(1.0).unwrap();

    // 6. Audio-only toggle: video track off, then back on, no error and
    //    playback continues.
    player.set_audio_only(true).unwrap();
    thread::sleep(Duration::from_millis(500));
    player.set_audio_only(false).unwrap();
    let after = wait_for_position(&player, Duration::from_secs(5));
    assert!(after > 0.0, "playback stalled after audio-only toggle");
}
