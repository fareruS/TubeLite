//! Player crate: queue logic (pure, always compiled) + mpv backend
//! (feature `mpv` only — see `mpv_backend.rs`).
//!
//! The queue, resume and quality-switch state machines are pure Rust and
//! are tested offline. The mpv backend is the only place that touches
//! libmpv, and it is not compiled in the default build.
//!
//! Stage 1 (see `resolver.rs` + `player.rs`): yt-dlp resolution
//! (`Resolver` / `YtDlpResolver`) and the `Player`, which loads the
//! resolved DASH pair into mpv through the `MpvControl` trait and
//! recovers from URL failures by re-resolving once. The mpv side of
//! that trait is `mpv_control::MpvControlImpl` (feature `mpv` only);
//! everything else is testable offline with fakes.

pub mod config;
pub mod events;
pub mod playback;
pub mod player;
pub mod queue;
pub mod resolver;

#[cfg(feature = "mpv")]
pub mod mpv_backend;
#[cfg(feature = "mpv")]
pub mod mpv_control;
#[cfg(feature = "mpv")]
pub mod service;

pub use config::MpvConfig;
pub use events::{PlayerEvent, PlayerState};
#[cfg(feature = "mpv")]
pub use mpv_backend::MpvPlayer;
#[cfg(feature = "mpv")]
pub use mpv_control::MpvControlImpl;
pub use playback::{PlaybackState, UrlFailureAction};
pub use player::{MpvControl, MpvEvent, Player, PlayerError};
pub use queue::{Queue, QueueItem, QueueSnapshot, RepeatMode};
pub use resolver::{
    redact_secrets, ResolveError, Resolved, Resolver, YtDlpResolver, YT_DLP_UPDATE_COMMAND,
};
#[cfg(feature = "mpv")]
pub use service::PlayerService;
