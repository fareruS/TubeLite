use async_trait::async_trait;
use yt_model::{
    Capabilities, ChannelId, ChannelPage, Cursor, Page, PlayableMedia, PlaylistId, QualityPref,
    VideoCard, VideoDetails, VideoId,
};

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub kind: SearchKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    All,
    Videos,
    Channels,
    Playlists,
}

#[async_trait]
#[allow(clippy::double_must_use)]
pub trait StreamSource: Send + Sync {
    async fn search(&self, q: &SearchQuery) -> Result<Page<VideoCard>>;
    async fn video(&self, id: &VideoId) -> Result<VideoDetails>;
    async fn channel(&self, id: &ChannelId) -> Result<ChannelPage>;
    async fn playlist(&self, id: &PlaylistId, cursor: Option<Cursor>) -> Result<Page<VideoCard>>;
    async fn playable(&self, id: &VideoId, pref: QualityPref) -> Result<PlayableMedia>;
    fn capabilities(&self) -> Capabilities;
    fn name(&self) -> &'static str;
}
