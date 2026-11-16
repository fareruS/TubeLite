//! `StreamSource` — the single seam behind which everything fragile lives.
//!
//! Implementations:
//! - `FixtureSource` (Phase 1, offline): parses the JSON fixtures in
//!   `tests/fixtures/`. This is what `cargo test` uses. No network.
//! - `InnerTubeSource` (Phase 3): pure-Rust InnerTube client. Not yet
//!   implemented — see `docs/PLAN.md`. Its client identities live in
//!   `clients.json` + an on-disk override, never hard-coded in ten places.
//! - `ExtractorSource` (Phase 6): yt-dlp sidecar, hardened subprocess.
//! - `OfficialSource` (Phase 6): YouTube Data API v3, user-supplied key.
//!
//! All implementations must pass the same contract tests in
//! `tests/contract.rs` (against fixtures or a mock server).

pub mod captions;
pub mod clients;
pub mod error;
pub mod extractor;
pub mod fixture;
pub mod innertube;
pub mod innertube_parse;
pub mod innertube_source;
pub mod traits;

pub use captions::{fetch_captions, parse_json3, parse_srv3, to_vtt, CaptionCue};
pub use clients::{ClientIdentity, ClientRegistry};
pub use error::SourceError;
pub use extractor::ExtractorSource;
pub use fixture::FixtureSource;
pub use innertube::{FailureMemory, FallbackChain};
pub use innertube_source::{parse_player_response, InnerTubeSource};
pub use traits::StreamSource;
