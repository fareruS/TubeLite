//! The feed functions: one per feed, each issuing the request
//! shape found in the matching fixture (see
//! `docs/renderer-map.md`) and returning a [`FeedPage`].
//!
//! Request shapes (all `POST`, client `WEB`):
//!
//! | function | endpoint | body |
//! |---|---|---|
//! | `home` | `browse` | `{context, browseId: "FEwhat_to_watch", inlineSettingStatus: "INLINE_SETTING_STATUS_ON"}` |
//! | `subscriptions` | `browse` | `{context, browseId: "FEsubscriptions"}` |
//! | `history` | `browse` | `{context, browseId: "FEhistory"}` |
//! | `library_playlists` | `browse` | `{context, browseId: "FEplaylist_aggregation"}` (from the guide fixture) |
//! | `playlist` | `browse` | `{context, browseId: <id>}` |
//! | `watch_later` | `browse` | `{context, browseId: "VLWL"}` (from the guide fixture) |
//! | `home_continuation` | `browse` | `{context, continuation: <token>}` — no browseId |
//! | `search` | `search` | `{context, query, inlineSettingStatus: "INLINE_SETTING_STATUS_ON"}` (+ `params` when a filter is given) |
//! | `channel_videos` | `browse` | `{context, browseId: <UC…>, params: "EgZ2aWRlb3PyBgQKAjoA"}` (Videos-tab params from the channel response fixture) |
//! | `watch_next` | `get_watch` | `{context, playerRequest: {videoId}, watchNextRequest: {videoId}}` |
//!
//! Notes on fidelity:
//! - The search fixture also carries `webSearchboxStatsUrl`, a
//!   browser-generated stats URL with an opaque session token. It
//!   cannot be reproduced for a new query, so it is omitted.
//! - The `get_watch` capture carries extra playback fields
//!   (`params`, `playbackContext`, `serviceIntegrityDimensions`)
//!   that are session-specific; the minimal `{videoId}` shape is
//!   sent instead.
//! - The captured channel request had a client-only `context` (a
//!   client-side navigation artifact); the standard full context is
//!   sent, which `/browse` accepts (proven by home/subscriptions/
//!   history).
//! - `playlist`, `watch_later`, `library_playlists`, and
//!   `channel_videos` have NO response fixture: their browseIds and
//!   request shapes are grounded (guide + channel fixtures), but the
//!   response shape is unverified. They parse with the generic
//!   dispatcher, so whatever known renderers appear are extracted
//!   and the rest are counted as skipped.
//! - `search_suggestions` has no endpoint in any fixture and
//!   returns [`Error::MissingFixture`] naming the capture needed.

use std::sync::Arc;

use innertube::Innertube;
use serde_json::{json, Value};

use crate::error::Error;
use crate::model::FeedPage;
use crate::parse::{parse_feed, parse_watch_next};

/// The client for the `WEB` identity.
const CLIENT: &str = "WEB";

/// The Videos-tab `params` from the channel response fixture
/// (identical in both channel fixtures).
const CHANNEL_VIDEOS_PARAMS: &str = "EgZ2aWRlb3PyBgQKAjoA";

/// The Watch Later browse id, from the guide fixture.
const WATCH_LATER_BROWSE_ID: &str = "VLWL";

/// The Library playlists browse id, from the guide fixture.
const LIBRARY_PLAYLISTS_BROWSE_ID: &str = "FEplaylist_aggregation";

/// The feed client: a thin layer over [`Innertube`].
pub struct Feeds {
    inner: Arc<Innertube>,
}

impl Feeds {
    /// Wrap an [`Innertube`] client.
    pub fn new(inner: Arc<Innertube>) -> Self {
        Self { inner }
    }

    async fn context(&self) -> Result<Value, Error> {
        Ok(self.inner.context(CLIENT).await?)
    }

    async fn browse(&self, body: Value) -> Result<FeedPage, Error> {
        let value = self.inner.post("browse", CLIENT, body).await?;
        Ok(parse_feed(&value))
    }

    /// The home feed (`FEwhat_to_watch`).
    pub async fn home(&self) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        self.browse(json!({
            "context": context,
            "browseId": "FEwhat_to_watch",
            "inlineSettingStatus": "INLINE_SETTING_STATUS_ON",
        }))
        .await
    }

    /// The next home page for a continuation token from a previous
    /// home page. The token is sent bare (`{context, continuation}`),
    /// with no browseId, exactly as the capture does.
    pub async fn home_continuation(&self, token: &str) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        self.browse(json!({
            "context": context,
            "continuation": token,
        }))
        .await
    }

    /// The subscriptions feed (`FEsubscriptions`).
    pub async fn subscriptions(&self) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        self.browse(json!({
            "context": context,
            "browseId": "FEsubscriptions",
        }))
        .await
    }

    /// The history feed (`FEhistory`).
    pub async fn history(&self) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        self.browse(json!({
            "context": context,
            "browseId": "FEhistory",
        }))
        .await
    }

    /// The Library playlists page (`FEplaylist_aggregation`, from the
    /// guide fixture). Response shape unverified — see the module
    /// docs.
    pub async fn library_playlists(&self) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        self.browse(json!({
            "context": context,
            "browseId": LIBRARY_PLAYLISTS_BROWSE_ID,
        }))
        .await
    }

    /// A playlist page. `browse_id` is the playlist browse id (e.g.
    /// `VLWL` for Watch Later, `PL…` for a playlist — the `VL`
    /// prefix form the guide uses). Response shape unverified — see
    /// the module docs.
    pub async fn playlist(&self, browse_id: &str) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        self.browse(json!({
            "context": context,
            "browseId": browse_id,
        }))
        .await
    }

    /// The Watch Later page (`VLWL`, from the guide fixture).
    /// Response shape unverified — see the module docs.
    pub async fn watch_later(&self) -> Result<FeedPage, Error> {
        self.playlist(WATCH_LATER_BROWSE_ID).await
    }

    /// Search. `filter_params` is the opaque `params` value from a
    /// captured filtered search, or `None` for the plain search in
    /// the fixture. No filter `params` values were ever captured, so
    /// they are passed through, never invented.
    pub async fn search(
        &self,
        query: &str,
        filter_params: Option<&str>,
    ) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        let mut body = json!({
            "context": context,
            "query": query,
            "inlineSettingStatus": "INLINE_SETTING_STATUS_ON",
        });
        if let Some(params) = filter_params {
            body["params"] = json!(params);
        }
        let value = self.inner.post("search", CLIENT, body).await?;
        // A search response without `contents` is not a genuine
        // zero-result page — a real empty search still carries
        // `contents` with a message renderer. Without it the
        // response is an error/empty payload (rate limit, challenge)
        // that must surface as an error, not a silent empty page
        // the user cannot tell apart from "no results".
        if value.get("contents").is_none() {
            return Err(Error::UnexpectedShape(
                "search response has no contents; YouTube returned an empty or error payload"
                    .to_string(),
            ));
        }
        Ok(parse_feed(&value))
    }

    /// Search suggestions. There is no suggestions endpoint in any
    /// fixture, request file, or HAR, so this cannot be implemented
    /// without inventing one. Capture a `searchbox` /
    /// `get_search_suggestions` request (DevTools Network, youtubei
    /// filter, type in the search box) and add its endpoint and body
    /// shape to `fixtures/` first.
    pub async fn search_suggestions(&self, _prefix: &str) -> Result<FeedPage, Error> {
        Err(Error::MissingFixture(
            "no search-suggestions endpoint in any fixture; capture a searchbox/get_search_suggestions request (DevTools Network, youtubei filter, type in the search box) and add its endpoint and body shape to fixtures/"
                .to_string(),
        ))
    }

    /// A channel's Videos tab. The request shape is grounded (the
    /// `params` value comes from the channel response fixture), but
    /// the response shape is unverified — no fixture loads the Videos
    /// tab. Items are parsed with the generic dispatcher.
    pub async fn channel_videos(&self, channel_id: &str) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        self.browse(json!({
            "context": context,
            "browseId": channel_id,
            "params": CHANNEL_VIDEOS_PARAMS,
        }))
        .await
    }

    /// The Up Next list for a video, from `get_watch`. The response
    /// is a two-item array `[playerResponse, watchNextResponse]`; the
    /// page comes from the `watchNextResponse` Up Next list. The
    /// player response is not part of the page (playback data belongs
    /// to the player).
    pub async fn watch_next(&self, video_id: &str) -> Result<FeedPage, Error> {
        let context = self.context().await?;
        let body = json!({
            "context": context,
            "playerRequest": {"videoId": video_id},
            "watchNextRequest": {"videoId": video_id},
        });
        let value = self.inner.post("get_watch", CLIENT, body).await?;
        parse_watch_next(&value)
    }
}
