//! The feed data model: one video item, one page of items, and the
//! parse statistics. Plain data, no I/O.

use serde::{Deserialize, Serialize};

/// One thumbnail image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thumb {
    /// The image URL.
    pub url: String,
    /// Width in pixels, when the fixture gives it.
    pub width: Option<u32>,
    /// Height in pixels, when the fixture gives it.
    pub height: Option<u32>,
}

/// One badge on a video (e.g. the "Verified" check on a channel).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Badge {
    /// The badge text (`label`, or `tooltip` when there is no label).
    pub label: String,
    /// The badge style, e.g. `BADGE_STYLE_TYPE_VERIFIED`.
    pub style: Option<String>,
}

/// One filter chip (home, search, watch-next).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chip {
    /// The chip text, e.g. `All`.
    pub text: String,
    /// The chip's continuation token, when it has one. The selected
    /// chip has no token (it does not navigate anywhere).
    pub token: Option<String>,
    /// Whether this chip is the selected one.
    pub selected: bool,
}

/// One video in a feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedItem {
    /// The video id.
    pub video_id: String,
    /// The video title.
    pub title: String,
    /// The channel name (empty when the node does not name one — a
    /// channel shelf item, for example).
    pub channel_name: String,
    /// The channel id (`UC…`, empty when unknown).
    pub channel_id: String,
    /// Thumbnails, largest first as the fixture orders them.
    pub thumbnails: Vec<Thumb>,
    /// Duration text, e.g. `27:47` (empty when unknown).
    pub duration_text: String,
    /// Published text, e.g. `1mo ago` (empty when unknown).
    pub published_text: String,
    /// View count text, e.g. `1M` or `167,488 views` (empty when
    /// unknown).
    pub view_count_text: String,
    /// Badges, e.g. `Verified`.
    pub badges: Vec<Badge>,
}

/// How much of a response the parser attempted and skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ParseStats {
    /// Video-item nodes the dispatcher attempted
    /// (`videoRenderer` + `lockupViewModel` nodes found).
    pub attempted: usize,
    /// Of those, how many could not be parsed into a `FeedItem`
    /// (non-video lockups, ads, missing id/title). Skipped items are
    /// never an error.
    pub skipped: usize,
}

/// One page of a feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedPage {
    /// The parsed video items, in document order.
    pub items: Vec<FeedItem>,
    /// The continuation token for the next page, when the response
    /// carries one.
    pub continuation: Option<String>,
    /// The filter chips, when the response carries a chip bar.
    pub chips: Vec<Chip>,
    /// Parse statistics (see [`ParseStats`]).
    pub stats: ParseStats,
}
