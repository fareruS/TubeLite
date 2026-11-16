//! Source selection and chaining — the app crate's job.
//!
//! The UI never chooses a source; it calls IPC commands, and the app
//! crate resolves them against the configured chain:
//!
//! - `source_mode = "fixture"` (default): `FixtureSource` — offline demo
//!   data. Safe default while live InnerTube is unverified on this VM's
//!   egress (see KNOWN-ISSUES.md).
//! - `source_mode = "innertube"`: `InnerTubeSource` primary; if the
//!   `ytdlp_path` setting names a sidecar binary, `ExtractorSource` is
//!   the fallback for `playable()` only (it cannot search or browse).
//!
//! Fallback rule for `playable()`: on Cipher / Forbidden / Network /
//! UrlExpired from the primary, try the extractor once; any other error
//! (NotFound, UnexpectedShape, Parse) is returned immediately — a
//! missing video will not be found by another source either.
//! Search / video / channel / playlist never fall back to the extractor
//! (it returns Unsupported for those by design).

use async_trait::async_trait;
use yt_model::{
    Capabilities, ChannelId, ChannelPage, Cursor, Page, PlayableMedia, PlaylistId, QualityPref,
    VideoCard, VideoDetails, VideoId,
};
use yt_source::{
    error::Result,
    traits::{SearchQuery, StreamSource},
    ClientRegistry, ExtractorSource, FixtureSource, InnerTubeSource, SourceError,
};

/// The live chain's fields, boxed so the enum variants stay close in
/// size (clippy::large_enum_variant).
pub struct LiveChain {
    primary: InnerTubeSource,
    extractor: Option<ExtractorSource>,
}

/// The active source chain. Held in `AppState`; built once at startup
/// from the store's settings.
pub enum AnySource {
    Fixture(FixtureSource),
    Live(Box<LiveChain>),
}

/// Errors worth one fallback attempt to the extractor. Anything else
/// (NotFound, UnexpectedShape, Parse) is returned immediately — a
/// missing video will not be found by another source either.
fn should_fallback(e: &SourceError) -> bool {
    matches!(
        e,
        SourceError::Cipher
            | SourceError::Forbidden
            | SourceError::Network(_)
            | SourceError::UrlExpired
    )
}

impl AnySource {
    /// Build the chain from settings. `source_mode` is "fixture" unless
    /// set to "innertube"; `ytdlp_path` enables the extractor fallback.
    /// A failure to build the live source (e.g. bad client registry)
    /// falls back to the fixture source rather than failing startup —
    /// the UI shows the limitation note from `capabilities()`.
    pub fn from_settings(source_mode: Option<String>, ytdlp_path: Option<String>) -> Self {
        if source_mode.as_deref() == Some("innertube") {
            match InnerTubeSource::new(ClientRegistry::bundled()) {
                Ok(primary) => {
                    let extractor = ytdlp_path
                        .filter(|p| !p.trim().is_empty())
                        .map(ExtractorSource::new);
                    return Self::Live(Box::new(LiveChain { primary, extractor }));
                }
                Err(e) => {
                    tracing::warn!("failed to build InnerTubeSource ({e}); using fixture source");
                }
            }
        }
        Self::Fixture(FixtureSource)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Fixture(_) => "fixture",
            Self::Live { .. } => "innertube",
        }
    }
}

#[async_trait]
#[allow(clippy::double_must_use)]
impl StreamSource for AnySource {
    async fn search(&self, q: &SearchQuery) -> Result<Page<VideoCard>> {
        match self {
            Self::Fixture(s) => s.search(q).await,
            Self::Live(chain) => chain.primary.search(q).await,
        }
    }

    async fn video(&self, id: &VideoId) -> Result<VideoDetails> {
        match self {
            Self::Fixture(s) => s.video(id).await,
            Self::Live(chain) => chain.primary.video(id).await,
        }
    }

    async fn channel(&self, id: &ChannelId) -> Result<ChannelPage> {
        match self {
            Self::Fixture(s) => s.channel(id).await,
            Self::Live(chain) => chain.primary.channel(id).await,
        }
    }

    async fn playlist(&self, id: &PlaylistId, cursor: Option<Cursor>) -> Result<Page<VideoCard>> {
        match self {
            Self::Fixture(s) => s.playlist(id, cursor).await,
            Self::Live(chain) => chain.primary.playlist(id, cursor).await,
        }
    }

    async fn playable(&self, id: &VideoId, pref: QualityPref) -> Result<PlayableMedia> {
        match self {
            Self::Fixture(s) => s.playable(id, pref).await,
            Self::Live(chain) => match chain.primary.playable(id, pref).await {
                Ok(media) => Ok(media),
                Err(e) if should_fallback(&e) => match &chain.extractor {
                    Some(ext) => ext.playable(id, pref).await,
                    None => Err(e),
                },
                Err(e) => Err(e),
            },
        }
    }

    fn capabilities(&self) -> Capabilities {
        match self {
            Self::Fixture(s) => s.capabilities(),
            Self::Live(chain) => {
                let mut caps = chain.primary.capabilities();
                caps.limitation_note = Some(match &chain.extractor {
                    Some(_) => "Live InnerTube with yt-dlp fallback for streams.".into(),
                    None => "Live InnerTube (no yt-dlp fallback configured).".into(),
                });
                caps
            }
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Fixture(_) => "fixture",
            Self::Live { .. } => "innertube",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_settings_defaults_to_fixture() {
        let s = AnySource::from_settings(None, None);
        assert_eq!(s.name(), "fixture");
        let s = AnySource::from_settings(Some("bogus".into()), None);
        assert_eq!(s.name(), "fixture");
    }

    #[test]
    fn from_settings_innertube_builds_live_chain() {
        let s = AnySource::from_settings(Some("innertube".into()), None);
        assert_eq!(s.name(), "innertube");
        assert!(s.capabilities().limitation_note.is_some());
        // With an extractor path, the note says so.
        let s = AnySource::from_settings(Some("innertube".into()), Some("/usr/bin/yt-dlp".into()));
        assert!(s
            .capabilities()
            .limitation_note
            .unwrap()
            .contains("yt-dlp fallback"));
    }

    #[test]
    fn fallback_classification() {
        assert!(should_fallback(&SourceError::Cipher));
        assert!(should_fallback(&SourceError::Forbidden));
        assert!(should_fallback(&SourceError::Network("x".into())));
        assert!(should_fallback(&SourceError::UrlExpired));
        assert!(!should_fallback(&SourceError::NotFound("x".into())));
        assert!(!should_fallback(&SourceError::UnexpectedShape("x".into())));
        assert!(!should_fallback(&SourceError::Parse("x".into())));
    }

    #[tokio::test]
    async fn fixture_chain_still_serves_search() {
        let s = AnySource::from_settings(None, None);
        let page = s
            .search(&SearchQuery {
                text: "fixture".into(),
                kind: yt_source::traits::SearchKind::All,
            })
            .await
            .unwrap();
        assert!(!page.items.is_empty());
    }
}
