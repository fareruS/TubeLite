//! Stage 1 player: resolve a video id through a [`Resolver`], load
//! the resulting DASH pair into mpv, and recover from URL failures.
//!
//! The mpv side is behind the [`MpvControl`] trait so the recovery
//! logic is testable with a fake mpv (see `tests/player_recovery.rs`).
//! The real adapter lives in [`mpv_control`](crate::mpv_control)
//! (feature `mpv` only).
//!
//! Concurrency: `Player` is cheaply cloneable (`Arc` over the inner
//! state). Commands (`play`/`pause`/`seek`/`set_quality`) and the
//! event loop (`run_event_loop`, run as a background task) all take
//! `&self`. The control lock is held only for single mpv calls, and
//! resolution never holds a lock — a re-resolve takes seconds and
//! must not block `pause()`.
//!
//! Recovery rule (spec): on end-file with an error, or a 403 in the
//! mpv log, re-resolve once and resume at the saved position. A
//! second failure for the same item is terminal: state `Error`, no
//! further re-resolve.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{broadcast, Mutex};

use crate::events::{PlayerEvent, PlayerState};
use crate::resolver::{ResolveError, Resolved, Resolver};

/// mpv events, translated from libmpv2's [`Event`](libmpv2::events::Event)
/// by the real adapter (or scripted by a fake in tests). Only the
/// shapes the player reacts to are represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvEvent {
    /// The file ended with an error (e.g. an expired/403 URL).
    EndFileError,
    /// The file ended naturally.
    EndFileEof,
    /// Playback was stopped (user command / `loadfile` replace).
    EndFileStopped,
    /// A 403 appeared in the mpv log.
    Log403,
    Seek,
    PlaybackRestart,
    Other,
}

/// The mpv surface the player needs. The real implementation wraps
/// [`MpvPlayer`](crate::MpvPlayer); tests substitute a fake.
pub trait MpvControl: Send {
    /// Load the video URL and attach the audio URL as an external
    /// audio track (`audio-file` / `audio-add`).
    fn load_pair(&mut self, video_url: &str, audio_url: Option<&str>) -> Result<(), PlayerError>;
    fn set_pause(&mut self, pause: bool) -> Result<(), PlayerError>;
    fn seek(&mut self, secs: f64) -> Result<(), PlayerError>;
    fn set_option(&mut self, key: &str, value: &str) -> Result<(), PlayerError>;
    /// Set the stage-1 options (see [`MpvConfig::stage1_options`](crate::MpvConfig::stage1_options))
    /// before any playback.
    fn apply_stage1_options(&mut self, cache_dir: &Path) -> Result<(), PlayerError>;
    fn time_pos(&self) -> Option<f64>;
    fn duration(&self) -> Option<f64>;
    /// Drain one event, waiting at most `timeout`. `None` on timeout.
    fn wait_event(&mut self, timeout: Duration) -> Option<MpvEvent>;
}

/// Errors from the player. No variant carries a cookie value.
#[derive(Debug, Clone, Error)]
pub enum PlayerError {
    #[error("resolve failed: {0}")]
    Resolve(#[from] ResolveError),
    #[error("mpv error: {0}")]
    Mpv(String),
    #[error("no video is loaded")]
    NoCurrentVideo,
}

/// The video currently loaded (or being recovered).
#[derive(Debug, Clone)]
struct Current {
    video_id: String,
    /// Whether the one recovery for this item has already been used.
    recovered: bool,
}

#[derive(Debug)]
struct Playback {
    state: PlayerState,
    current: Option<Current>,
    max_height: u32,
    /// Last known position (polled from mpv, or the last seek target).
    last_position: f64,
    last_emitted_position: f64,
    duration: Option<f64>,
}

struct Inner {
    resolver: Arc<dyn Resolver>,
    control: Mutex<Box<dyn MpvControl>>,
    state: Mutex<Playback>,
    events: broadcast::Sender<PlayerEvent>,
    stop: AtomicBool,
}

/// Broadcast channel capacity for player events (bounded by
/// construction; a lagging consumer drops old events).
const EVENT_CAPACITY: usize = 64;

/// How often the event loop polls mpv's position, and the smallest
/// position change worth emitting.
const POSITION_POLL: Duration = Duration::from_millis(100);
const POSITION_EPSILON: f64 = 0.25;

#[derive(Clone)]
pub struct Player {
    inner: Arc<Inner>,
}

impl Player {
    /// Build a player. Stage-1 options are applied to the control
    /// before any playback.
    pub fn new(
        resolver: Arc<dyn Resolver>,
        mut control: Box<dyn MpvControl>,
        cache_dir: PathBuf,
        max_height: u32,
    ) -> Result<Self, PlayerError> {
        control.apply_stage1_options(&cache_dir)?;
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Ok(Self {
            inner: Arc::new(Inner {
                resolver,
                control: Mutex::new(control),
                state: Mutex::new(Playback {
                    state: PlayerState::Idle,
                    current: None,
                    max_height,
                    last_position: 0.0,
                    last_emitted_position: f64::NEG_INFINITY,
                    duration: None,
                }),
                events,
                stop: AtomicBool::new(false),
            }),
        })
    }

    /// Subscribe to position/state events.
    pub fn subscribe(&self) -> broadcast::Receiver<PlayerEvent> {
        self.inner.events.subscribe()
    }

    /// Ask the event loop to stop (checked once per iteration).
    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }

    pub async fn state(&self) -> PlayerState {
        self.inner.state.lock().await.state.clone()
    }

    pub async fn position(&self) -> f64 {
        self.inner.state.lock().await.last_position
    }

    fn emit(&self, event: PlayerEvent) {
        let _ = self.inner.events.send(event);
    }

    /// Resolve and play a video from the start.
    pub async fn play(&self, video_id: &str) -> Result<(), PlayerError> {
        let max_height = self.inner.state.lock().await.max_height;
        self.set_state(PlayerState::Loading).await;
        let resolved = match self.inner.resolver.resolve(video_id, max_height).await {
            Ok(resolved) => resolved,
            Err(e) => {
                self.fail(format!("resolve failed: {e}")).await;
                return Err(PlayerError::Resolve(e));
            }
        };
        self.load_resolved(video_id, resolved, 0.0, false, false)
            .await?;
        Ok(())
    }

    /// Pause playback.
    pub async fn pause(&self) -> Result<(), PlayerError> {
        self.inner.control.lock().await.set_pause(true)?;
        self.set_state(PlayerState::Paused).await;
        Ok(())
    }

    /// Resume playback.
    pub async fn resume(&self) -> Result<(), PlayerError> {
        self.inner.control.lock().await.set_pause(false)?;
        self.set_state(PlayerState::Playing).await;
        Ok(())
    }

    /// Absolute seek, clamped to the known duration. Errors when
    /// nothing is loaded.
    pub async fn seek(&self, secs: f64) -> Result<(), PlayerError> {
        if self.inner.state.lock().await.current.is_none() {
            return Err(PlayerError::NoCurrentVideo);
        }
        let target = {
            let mut state = self.inner.state.lock().await;
            let clamped = secs.max(0.0);
            let clamped = match state.duration {
                Some(d) => clamped.min(d),
                None => clamped,
            };
            state.last_position = clamped;
            clamped
        };
        self.inner.control.lock().await.seek(target)?;
        self.emit(PlayerEvent::Position(target));
        Ok(())
    }

    /// Switch quality: re-resolve the current video at the new max
    /// height, reload, and resume at the saved position (keeping the
    /// pause state). Errors when nothing is loaded.
    pub async fn set_quality(&self, max_height: u32) -> Result<(), PlayerError> {
        let (video_id, position, was_paused) = {
            let state = self.inner.state.lock().await;
            let Some(current) = &state.current else {
                return Err(PlayerError::NoCurrentVideo);
            };
            (
                current.video_id.clone(),
                state.last_position,
                state.state == PlayerState::Paused,
            )
        };
        self.inner.state.lock().await.max_height = max_height;
        let resolved = self.inner.resolver.resolve(&video_id, max_height).await?;
        self.load_resolved(&video_id, resolved, position, was_paused, false)
            .await?;
        self.emit(PlayerEvent::QualityChanged(max_height));
        Ok(())
    }

    /// Load a resolved video at `position`, restoring the pause state.
    /// `position == 0.0` means "from the start" (a fresh `play`).
    /// `recovered` is true only for a recovery reload: the one
    /// recovery for this item has already been used, so a second
    /// failure for it is terminal.
    async fn load_resolved(
        &self,
        video_id: &str,
        resolved: Resolved,
        position: f64,
        paused: bool,
        recovered: bool,
    ) -> Result<(), PlayerError> {
        // Headers first: the stream request must carry them.
        let header_value = resolved
            .headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(",");
        {
            let mut control = self.inner.control.lock().await;
            if !header_value.is_empty() {
                control.set_option("http-header-fields", &header_value)?;
            }
            control.load_pair(&resolved.video_url, resolved.audio_url.as_deref())?;
            if position > 0.0 {
                control.seek(position)?;
            }
            control.set_pause(paused)?;
        }
        {
            let mut state = self.inner.state.lock().await;
            state.current = Some(Current {
                video_id: video_id.to_string(),
                recovered,
            });
            state.last_position = position;
            state.duration = resolved.duration;
            state.state = if paused {
                PlayerState::Paused
            } else {
                PlayerState::Playing
            };
        }
        self.emit(PlayerEvent::Duration(resolved.duration));
        self.emit(PlayerEvent::StateChanged(if paused {
            PlayerState::Paused
        } else {
            PlayerState::Playing
        }));
        if position > 0.0 {
            self.emit(PlayerEvent::Position(position));
        }
        Ok(())
    }

    async fn set_state(&self, state: PlayerState) {
        self.inner.state.lock().await.state = state.clone();
        self.emit(PlayerEvent::StateChanged(state));
    }

    async fn fail(&self, message: String) {
        self.inner.state.lock().await.state = PlayerState::Error(message.clone());
        self.emit(PlayerEvent::Error(message.clone()));
        self.emit(PlayerEvent::StateChanged(PlayerState::Error(message)));
    }

    /// The event pump. Run as a background task (`tokio::spawn`) and
    /// stop it with [`Player::stop`]. Drains mpv events, polls the
    /// position, and runs recovery on URL failures.
    pub async fn run_event_loop(&self) {
        loop {
            if self.inner.stop.load(Ordering::Relaxed) {
                return;
            }
            let event = {
                let mut control = self.inner.control.lock().await;
                control.wait_event(POSITION_POLL)
            };
            match event {
                Some(MpvEvent::EndFileError) | Some(MpvEvent::Log403) => {
                    self.handle_url_failure().await;
                }
                Some(MpvEvent::EndFileEof) => {
                    self.inner.state.lock().await.state = PlayerState::Ended;
                    self.emit(PlayerEvent::Ended);
                    self.emit(PlayerEvent::StateChanged(PlayerState::Ended));
                }
                Some(MpvEvent::EndFileStopped)
                | Some(MpvEvent::Seek)
                | Some(MpvEvent::PlaybackRestart)
                | Some(MpvEvent::Other)
                | None => {}
            }
            self.poll_position().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Poll mpv's clock into the state and emit position/duration
    /// events when they move.
    async fn poll_position(&self) {
        let playing = self.inner.state.lock().await.state == PlayerState::Playing;
        if !playing {
            return;
        }
        let (pos, dur) = {
            let control = self.inner.control.lock().await;
            (control.time_pos(), control.duration())
        };
        let mut state = self.inner.state.lock().await;
        let mut emit_pos = None;
        let mut emit_dur = None;
        if let Some(pos) = pos {
            state.last_position = pos;
            if (pos - state.last_emitted_position).abs() >= POSITION_EPSILON {
                state.last_emitted_position = pos;
                emit_pos = Some(pos);
            }
        }
        if dur != state.duration {
            state.duration = dur;
            emit_dur = Some(dur);
        }
        drop(state);
        if let Some(pos) = emit_pos {
            self.emit(PlayerEvent::Position(pos));
        }
        if let Some(dur) = emit_dur {
            self.emit(PlayerEvent::Duration(dur));
        }
    }

    /// One URL failure: re-resolve once and resume at the saved
    /// position. A second failure for the same item is terminal.
    async fn handle_url_failure(&self) {
        let snapshot = {
            let mut state = self.inner.state.lock().await;
            let Some(current) = state.current.clone() else {
                return;
            };
            if current.recovered {
                drop(state);
                self.fail("stream failed after re-resolve".to_string())
                    .await;
                return;
            }
            state.current.as_mut().expect("current checked").recovered = true;
            (
                current.video_id.clone(),
                state.max_height,
                state.last_position,
                state.state == PlayerState::Paused,
            )
        };
        let (video_id, max_height, position, was_paused) = snapshot;
        self.emit(PlayerEvent::Recovering { attempt: 1 });
        match self.inner.resolver.resolve(&video_id, max_height).await {
            Ok(resolved) => {
                match self
                    .load_resolved(&video_id, resolved, position, was_paused, true)
                    .await
                {
                    Ok(()) => {
                        self.emit(PlayerEvent::Recovered);
                    }
                    Err(e) => self.fail(format!("recovery reload failed: {e}")).await,
                }
            }
            Err(e) => self.fail(format!("recovery re-resolve failed: {e}")).await,
        }
    }
}
