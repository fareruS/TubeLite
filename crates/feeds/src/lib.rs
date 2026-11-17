//! Feed fetching and parsing on top of [`innertube`], driven
//! entirely by the captured fixtures in `fixtures/`.
//!
//! Two video-item generations occur in the fixtures (see
//! `docs/renderer-map.md`): the classic `videoRenderer` (search
//! only) and the new `lockupViewModel` (home, subscriptions, history,
//! watch-next, channel shelves). The dispatcher in [`extract`]
//! tries each generation per node; an unparseable item is skipped
//! and counted in [`ParseStats`], never an error.
//!
//! What is grounded, and what is not:
//! - Grounded: home, subscriptions, history, search, channel
//!   (Home tab), watch-next, and continuation — request shapes and
//!   response shapes both come from fixtures.
//! - Request grounded, response unverified: `playlist`,
//!   `watch_later`, `library_playlists` (browseIds from the guide
//!   fixture), and `channel_videos` (Videos-tab params from the
//!   channel response fixture). They parse with the generic
//!   dispatcher.
//! - No fixture at all: `search_suggestions` (see
//!   [`Error::MissingFixture`]) and the RSS sample (written for the
//!   tests from the public feed format).

mod channel;
mod continuation;
mod error;
mod extract;
mod feeds;
mod find;
mod model;
mod parse;
mod rss;

pub use channel::parse_channel_header;
pub use continuation::continuation_token;
pub use error::Error;
pub use extract::{extract_item, extract_lockup, extract_video_renderer, has_item_renderer};
pub use feeds::Feeds;
pub use find::find_all;
pub use model::{Badge, ChannelHeader, Chip, FeedItem, FeedPage, ParseStats, Thumb};
pub use parse::{chips_of, collect_items, parse_feed, parse_watch_next};
pub use rss::{fetch_rss, merge_by_published, parse_rss};
