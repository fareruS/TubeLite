//! Player events: position ticks and state changes, delivered over a
//! bounded tokio broadcast channel (`Player::subscribe`). The channel
//! capacity is 64; a slow consumer drops old events (broadcast lag is
//! surfaced as `RecvError::Lagged` on the receiver side, never as an
//! unbounded queue).

use serde::Serialize;

/// The player's high-level state, as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PlayerState {
    /// Nothing loaded.
    Idle,
    /// Resolving URLs / loading the file.
    Loading,
    Playing,
    Paused,
    /// The file ended naturally.
    Ended,
    /// Playback failed (after recovery was already tried once, or the
    /// failure was not a URL failure). The message is user-facing and
    /// never contains a cookie value.
    Error(String),
}

/// Events emitted by [`Player`](crate::Player) on its broadcast channel.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum PlayerEvent {
    /// Current position in seconds (emitted ~4 Hz while playing, and
    /// immediately after a seek).
    Position(f64),
    /// Duration became known (or changed after a re-resolve).
    Duration(Option<f64>),
    /// The high-level state changed.
    StateChanged(PlayerState),
    /// The file ended naturally.
    Ended,
    /// A URL failure was detected; the player is re-resolving once
    /// and will resume at the saved position.
    Recovering { attempt: u32 },
    /// The re-resolve succeeded and playback resumed.
    Recovered,
    /// Quality changed (the new max height). Emitted after the
    /// re-resolve + reload for the new quality.
    QualityChanged(u32),
    /// A non-recoverable playback error.
    Error(String),
}
