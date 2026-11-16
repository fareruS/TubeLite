# Renderer map — captured fixtures only

Source of truth: `fixtures/redacted/*.response.json` (captured 2026-11-16,
signed-in desktop browser, WEB client). Every renderer/field name below
occurs in at least one of those files. Nothing here is guessed from
documentation or other clients. Counts are occurrences in the named
fixture. Paths use `[]` for array indices.

Request shapes (from `fixtures/redacted/*.request.json`):

| feed | endpoint | body |
|---|---|---|
| home | `POST /youtubei/v1/browse` | `{context, browseId: "FEwhat_to_watch", inlineSettingStatus}` |
| subscriptions | `POST /youtubei/v1/browse` | `{context, browseId: "FEsubscriptions"}` |
| history | `POST /youtubei/v1/browse` | `{context, browseId: "FEhistory"}` |
| search | `POST /youtubei/v1/search` | `{context, query, inlineSettingStatus, webSearchboxStatsUrl}` |
| channel | `POST /youtubei/v1/browse` | `{context, browseId: "<UC…>", param: ""}` (Home tab; see below) |
| watch-next | `POST /youtubei/v1/get_watch` | `{context, playerRequest, watchNextRequest}` (videoId in **both** `playerRequest.videoId` and `watchNextRequest.videoId`) |
| continuation | `POST /youtubei/v1/browse` | `{context, continuation}` — no browseId |
| guide | `POST /youtubei/v1/guide` | `{context, fetchLiveState}` |

## Item generations (what a `FeedItem` extractor must handle)

Only two video-item generations occur in the fixtures. A dispatcher that
tries both per node covers every feed.

### Gen 1 — `videoRenderer` (classic). Occurs in: **search only** (n=18).

`contents.twoColumnSearchResultsRenderer.primaryContents.sectionListRenderer.contents[].itemSectionRenderer.contents[].videoRenderer`

| field | path inside `videoRenderer` |
|---|---|
| video id | `videoId` |
| title | `title.simpleText` / `title.runs[].text` |
| channel name | `ownerText` / `longBylineText` (same text); channel id is in `longBylineText.runs[].navigationEndpoint.browseEndpoint.browseId` |
| thumbnails | `thumbnail.thumbnails[]` (`url`, `width`, `height`) |
| duration | `lengthText.simpleText`; also `thumbnailOverlays[].thumbnailOverlayTimeStatusRenderer.text` |
| published | `publishedTimeText.simpleText` |
| views | `viewCountText.simpleText` (also `shortViewCountText.simpleText`) |
| badges | `ownerBadges[].metadataBadgeRenderer` |
| channel avatar | `channelThumbnailSupportedRenderers.channelThumbnailWithLinkRenderer.thumbnail.thumbnails[]` |
| chapters | `expandableMetadata.expandableMetadataRenderer.expandedContent.horizontalCardListRenderer.cards[].macroMarkersListItemRenderer` |

### Gen 2 — `lockupViewModel` (new). Occurs in: **home, subscriptions, history, watch-next (Up Next), channel shelves**.

| field | path inside `lockupViewModel` |
|---|---|
| video id | `contentId` |
| type check | `contentType` = `LOCKUP_CONTENT_TYPE_VIDEO` |
| title | `metadata.lockupMetadataViewModel.title.content` (plain string, **not** `simpleText`/`runs`) |
| channel name, views, age | `metadata.lockupMetadataViewModel.metadata.contentMetadataViewModel.metadataRows[0].metadataParts[].text.content` — home: `[channel, short views, age]` (e.g. `["Visual Venture", "1M", "1mo ago"]`); history: `[channel, short views]` (age part absent) |
| channel id | `metadataRows[0].metadataParts[0].text.commandRuns[].onTap.innertubeCommand.browseEndpoint.browseId` |
| channel avatar | `metadata.lockupMetadataViewModel.image.decoratedAvatarViewModel.avatar.avatarViewModel.image.sources[]` |
| thumbnails | `contentImage.thumbnailViewModel.image.sources[]` (`url`, `width`, `height`) |
| duration | `contentImage.thumbnailViewModel.overlays[].thumbnailBottomOverlayViewModel.badges[].thumbnailBadgeViewModel.text` |

No `compactVideoRenderer`, no `gridRenderer`, no `playlistVideoRenderer`
occurs in any fixture.

### Shorts items (not `FeedItem`s; skip or handle separately)

| key | where |
|---|---|
| `shortsLockupViewModel` | home `richItemRenderer.content` (n=18), subscriptions (n=12), search `gridShelfViewModel.contents[]` |
| `reelShelfRenderer` | history `itemSectionRenderer.contents[]` (n=1), channel shelves (n=1) |

### End-screen items (watch-next only)

`endScreenVideoRenderer` (n=12) at
`[1].watchNextResponse.playerOverlays.playerOverlayRenderer.endScreen.watchNextEndScreenRenderer.results[].endScreenVideoRenderer` —
fields: `videoId`, `title`, `shortBylineText`, `lengthText`,
`lengthInSeconds`, `shortViewCountText`, `publishedTimeText`,
`thumbnailOverlays`.

## Home — `home.response.json` (`FEwhat_to_watch`)

Root: `contents.twoColumnBrowseResultsRenderer.tabs[].tabRenderer.content.richGridRenderer`

| renderer | n | example path |
|---|---|---|
| `richGridRenderer` | 1 | `contents.twoColumnBrowseResultsRenderer.tabs[].tabRenderer.content.richGridRenderer` |
| `richItemRenderer` | 42 | `…richGridRenderer.contents[].richItemRenderer` |
| `lockupViewModel` | 22 | `…richItemRenderer.content.lockupViewModel` |
| `shortsLockupViewModel` | 18 | `…richItemRenderer.content.shortsLockupViewModel` |
| `adSlotRenderer` | 2 | `…richItemRenderer.content.adSlotRenderer` |
| `richSectionRenderer` | 2 | `…richGridRenderer.contents[].richSectionRenderer` |
| `richShelfRenderer` | 2 | `…richSectionRenderer.content.richShelfRenderer` |
| `feedFilterChipBarRenderer` | 1 | `…richGridRenderer.header.feedFilterChipBarRenderer` |
| `chipCloudChipRenderer` | 16 | `…feedFilterChipBarRenderer.contents[].chipCloudChipRenderer` |
| `continuationItemRenderer` | 1 | `…richGridRenderer.contents[].continuationItemRenderer` |
| `ghostGridRenderer` | 1 | `…continuationItemRenderer.ghostCards.ghostGridRenderer` |
| `feedTabbedHeaderRenderer` | 1 | `header.feedTabbedHeaderRenderer` |

Chips (home): `All` (selected), `Music`, `Gaming`, `Mixes`, `Podcasts`,
`Computer Hardware`, `Experiments`, `Live`, `History`, `Computers`,
`Gadgets`, `Role-Playing Games`, `Science`, `Recently uploaded`,
`Watched`, `New to you`. Chip text is `chipCloudChipRenderer.text`
(`simpleText`/`runs`); each chip's filter token is at
`chipCloudChipRenderer.navigationEndpoint.continuationCommand.token`.

No `videoRenderer` in home.

## Subscriptions — `subscriptions.response.json` (`FEsubscriptions`)

Same root shape as home (`…richGridRenderer`).

| renderer | n | example path |
|---|---|---|
| `richGridRenderer` | 1 | `contents.twoColumnBrowseResultsRenderer.tabs[].tabRenderer.content.richGridRenderer` |
| `richItemRenderer` | 124 | `…richGridRenderer.contents[].richItemRenderer` |
| `lockupViewModel` | 112 | `…richItemRenderer.content.lockupViewModel` |
| `shortsLockupViewModel` | 12 | `…richItemRenderer.content.shortsLockupViewModel` |
| `richSectionRenderer` | 3 | `…richGridRenderer.contents[].richSectionRenderer` |
| `richShelfRenderer` | 2 | `…richSectionRenderer.content.richShelfRenderer` |
| `shelfRenderer` | 1 | `…richSectionRenderer.content.shelfRenderer` |
| `continuationItemRenderer` | 1 | `…richGridRenderer.contents[].continuationItemRenderer` |
| `ghostGridRenderer` | 1 | `…continuationItemRenderer.ghostCards.ghostGridRenderer` |
| `feedTabbedHeaderRenderer` | 1 | `header.feedTabbedHeaderRenderer` |

**No chip bar in subscriptions** — no `feedFilterChipBarRenderer` /
`chipCloudChipRenderer` occurs in this fixture. No `videoRenderer`.

## History — `history.response.json` (`FEhistory`)

Root: `contents.twoColumnBrowseResultsRenderer.tabs[].tabRenderer.content.sectionListRenderer`

| renderer | n | example path |
|---|---|---|
| `sectionListRenderer` | 1 | `…tabRenderer.content.sectionListRenderer` |
| `itemSectionRenderer` | 6 | `…sectionListRenderer.contents[].itemSectionRenderer` |
| `itemSectionHeaderRenderer` | 6 | `…itemSectionRenderer.header.itemSectionHeaderRenderer` |
| `lockupViewModel` | 196 | `…itemSectionRenderer.contents[].lockupViewModel` |
| `reelShelfRenderer` | 1 | `…itemSectionRenderer.contents[].reelShelfRenderer` |
| `continuationItemRenderer` | 1 | `…sectionListRenderer.contents[].continuationItemRenderer` |
| `browseFeedActionsRenderer` | 1 | `contents.twoColumnBrowseResultsRenderer.secondaryContents.browseFeedActionsRenderer` (search box / clear-history controls, not feed items) |
| `pageHeaderRenderer` | 1 | `header.pageHeaderRenderer` |

No `videoRenderer`, no chip bar in history.

## Search — `search.response.json` (query `rust programmming`, as typed)

Root: `contents.twoColumnSearchResultsRenderer.primaryContents.sectionListRenderer`

| renderer | n | example path |
|---|---|---|
| `twoColumnSearchResultsRenderer` | 1 | `contents.twoColumnSearchResultsRenderer` |
| `sectionListRenderer` | 1 | `…primaryContents.sectionListRenderer` |
| `itemSectionRenderer` | 1 | `…sectionListRenderer.contents[].itemSectionRenderer` |
| `videoRenderer` | 18 | `…itemSectionRenderer.contents[].videoRenderer` |
| `searchPyvRenderer` | 2 | `…itemSectionRenderer.contents[].searchPyvRenderer` (ads; skip) |
| `showingResultsForRenderer` | 1 | `…itemSectionRenderer.contents[].showingResultsForRenderer` |
| `gridShelfViewModel` | 3 | `…itemSectionRenderer.contents[].gridShelfViewModel` (Shorts shelf; items are `shortsLockupViewModel`) |
| `continuationItemRenderer` | 1 | `…sectionListRenderer.contents[].continuationItemRenderer` |
| `searchHeaderRenderer` | 1 | `header.searchHeaderRenderer` |
| `chipCloudRenderer` | 1 | `header.searchHeaderRenderer.chipBar.chipCloudRenderer` |
| `chipCloudChipRenderer` | 7 | `header.searchHeaderRenderer.chipBar.chipCloudRenderer.chips[].chipCloudChipRenderer` |
| `searchFilterOptionsDialogRenderer` | 1 | `header.searchHeaderRenderer.searchFilterButton…openPopupAction.popup.searchFilterOptionsDialogRenderer` |
| `searchFilterGroupRenderer` | 5 | `…searchFilterOptionsDialogRenderer.groups[].searchFilterGroupRenderer` |
| `searchFilterRenderer` | 25 | `…searchFilterGroupRenderer.filters[].searchFilterRenderer` |

Search chips: `All` (selected), `Shorts`, `Unwatched`, `Watched`,
`Videos`, `Recently uploaded`, `Live`. The filter dialog
(`searchFilterRenderer`) is separate from the chips. No `channelRenderer`
or `playlistRenderer` occurs in this fixture (all 18 results are videos).

## Channel — `channel.response.json`, `channel2.response.json`

Captured requests loaded the channel **Home** tab (`param: ""` in both
`channel.request.json` and `channel2.request.json`). No fixture shows a
loaded Videos tab.

Root: `contents.twoColumnBrowseResultsRenderer.tabs[].tabRenderer.content.sectionListRenderer`

| renderer | n (ch1 / ch2) | example path |
|---|---|---|
| `sectionListRenderer` | 3 / 3 | `…tabRenderer.content.sectionListRenderer` |
| `itemSectionRenderer` | 15 / 5 | `…sectionListRenderer.contents[].itemSectionRenderer` |
| `shelfRenderer` | 11 / 3 | `…itemSectionRenderer.contents[].shelfRenderer` |
| `horizontalListRenderer` | 11 / 3 | `…shelfRenderer.content.horizontalListRenderer` |
| `postRenderer` | 12 / 12 | `…horizontalListRenderer.items[].postRenderer` (community posts) |
| `lockupViewModel` | (in shelves) | `…horizontalListRenderer.items[].lockupViewModel` |
| `reelShelfRenderer` | 1 / 0 | `…itemSectionRenderer.contents[].reelShelfRenderer` |
| `channelVideoPlayerRenderer` | 1 / 0 | `…itemSectionRenderer.contents[].channelVideoPlayerRenderer` (channel trailer) |
| `channelMetadataRenderer` | 1 / 1 | `metadata.channelMetadataRenderer` |
| `pageHeaderRenderer` | 1 / 1 | `header.pageHeaderRenderer` |
| `expandableTabRenderer` | 1 / 1 | `contents.twoColumnBrowseResultsRenderer.tabs[].expandableTabRenderer` |

Tabs (plain-string `title` on `tabRenderer`; unselected tabs have
`content: null`). Observed in **both** channel fixtures, with identical
`params` across channels:

| tab | `browseEndpoint.params` |
|---|---|
| Home (selected in capture) | `EghmZWF0dXJlZPIGBAoCMgA%3D` |
| **Videos** | **`EgZ2aWRlb3PyBgQKAjoA`** |
| Shorts | `EgZzaG9ydHPyBgUKA5oBAA%3D%3D` |
| Live | `EgdzdHJlYW1z8gYECgJ6AA%3D%3D` |
| Shows | `EgVzaG93c_IGBAoCYgA%3D` |
| **Playlists** | **`EglwbGF5bGlzdHPyBgoKCEIGCgIQaCIA`** |
| Posts | `EgVwb3N0c_IGBAoCSgA%3D` |

`channel_videos()` request shape is therefore known
(`browseId: <UC…>`, `param: "EgZ2aWRlb3PyBgQKAjoA"`), but its **response
shape is unobserved** — no fixture loads the Videos tab. The response
presumably reuses the `richGridRenderer`/`lockupViewModel` generation,
but that is not in any fixture and must be verified with a new capture.

The two `continuationItemRenderer`s in the channel fixtures sit inside
the About/description engagement panels
(`header.pageHeaderRenderer.content.pageHeaderViewModel.…showEngagementPanelEndpoint…`),
not in the feed — there is **no feed continuation token** on the channel
Home tab.

## Watch-next — `watch-next.response.json` (`get_watch`)

Response is a **JSON array of two items**: `[0].playerResponse` (same
shape as `/player`) and `[1].watchNextResponse`.

`[1].watchNextResponse` — Up Next list:

| renderer | n | example path |
|---|---|---|
| `twoColumnWatchNextResults` | 1 | `[1].watchNextResponse.contents.twoColumnWatchNextResults` |
| `videoPrimaryInfoRenderer` | 1 | `…results.results.contents[].videoPrimaryInfoRenderer` |
| `videoSecondaryInfoRenderer` | 1 | `…results.results.contents[].videoSecondaryInfoRenderer` |
| `videoOwnerRenderer` | 1 | `…videoSecondaryInfoRenderer.owner.videoOwnerRenderer` |
| `expandableMetadataRenderer` | 2 | `…results.results.contents[].expandableMetadataRenderer` |
| `itemSectionRenderer` (comments) | — | `…results.results.contents[].itemSectionRenderer` (header: `commentsHeaderRenderer`; its `continuationItemRenderer` token is the **comments** continuation, len 78) |
| `secondaryResults` | 1 | `…contents.twoColumnWatchNextResults.secondaryResults.secondaryResults` |
| `relatedChipCloudRenderer` | 1 | `…secondaryResults.results[].itemSectionRenderer.header.relatedChipCloudRenderer` |
| `chipCloudRenderer` | 1 | `…relatedChipCloudRenderer.content.chipCloudRenderer` |
| `chipCloudChipRenderer` | 5 | `…chipCloudRenderer.chips[].chipCloudChipRenderer` |
| `lockupViewModel` | 20 | `…secondaryResults.results[].itemSectionRenderer.contents[].lockupViewModel` |
| `continuationItemRenderer` | 1 | `…secondaryResults.results[].itemSectionRenderer.contents[].continuationItemRenderer` |
| `endScreenVideoRenderer` | 12 | `…playerOverlays.playerOverlayRenderer.endScreen.watchNextEndScreenRenderer.results[].endScreenVideoRenderer` |
| `chapterRenderer` | 8 | `…playerOverlays.playerOverlayRenderer.decoratedPlayerBarRenderer…markersMap[].value.chapters[].chapterRenderer` |
| `structuredDescriptionContentRenderer` | 1 | `…engagementPanels[].engagementPanelSectionListRenderer.content.structuredDescriptionContentRenderer` (description panel) |
| `macroMarkersListRenderer` | 1 | `…engagementPanels[]…content.macroMarkersListRenderer` (chapters panel; items `macroMarkersListItemRenderer`, n=16) |

Up Next chips: `All` (selected), `From your search`, `From bigboxSWE`,
`Related`, `For you`.

## Continuation — `continuation.response.json`

Top-level keys: `responseContext`, `trackingParams`,
`onResponseReceivedActions`, `frameworkUpdates` — **no `contents`**.

| renderer | n | example path |
|---|---|---|
| `appendContinuationItemsAction` | 1 | `onResponseReceivedActions[].appendContinuationItemsAction` |
| `richItemRenderer` | 24 | `…appendContinuationItemsAction.continuationItems[].richItemRenderer` |
| `lockupViewModel` | 24 | `…richItemRenderer.content.lockupViewModel` |
| `continuationItemRenderer` | 1 | `…appendContinuationItemsAction.continuationItems[].continuationItemRenderer` |
| `ghostGridRenderer` | 1 | `…continuationItemRenderer.ghostCards.ghostGridRenderer` |

Same item generation as home; the next page's items arrive inside
`appendContinuationItemsAction.continuationItems[]`, not under `contents`.

## Guide — `guide.response.json`

Top-level keys: `responseContext`, `items`, `trackingParams`,
`userProfile`. (Not a feed, but the source of library browseIds.)

| renderer | n | example path |
|---|---|---|
| `guideSectionRenderer` | 5 | `items[].guideSectionRenderer` |
| `guideSubscriptionsSectionRenderer` | 1 | `items[].guideSubscriptionsSectionRenderer` |
| `guideEntryRenderer` | 129 | `items[].guideSectionRenderer.items[].guideEntryRenderer` |
| `guideCollapsibleEntryRenderer` | 3 | `items[].guideSubscriptionsSectionRenderer.items[].guideCollapsibleEntryRenderer` |
| `guideCollapsibleSectionEntryRenderer` | 2 | `…items[].guideCollapsibleSectionEntryRenderer` |

Entry label is `guideEntryRenderer.formattedTitle` (not `title`); the
browseId is at
`guideEntryRenderer.navigationEndpoint.browseEndpoint.browseId`.
Relevant entries:

| label | browseId | notes |
|---|---|---|
| Home | `FEwhat_to_watch` | |
| Shorts | — | `reelWatchEndpoint`, no browseId |
| Subscriptions | `FEsubscriptions` | |
| You | `FElibrary` | `params: "KgN5b3U%3D"`, url `/feed/you` |
| History | `FEhistory` | |
| Playlists | `FEplaylist_aggregation` | url `/feed/playlists` |
| **Watch later** | **`VLWL`** | url `/playlist?list=WL` |
| **Liked videos** | **`VLLL`** | url `/playlist?list=LL` |
| Downloads | `FEdownloads` | |
| Podcasts | `FEpodcasts` | |
| Clips | `FEclips` | |
| YouTube Premium | `SPunlimited` | |
| YouTube Labs | `SPyoutube_labs` | |
| Movies | `FEstorefront` | `params: "ogUCKAU%3D"` |
| Playables | `FEmini_app_destination` | |
| Memberships | `FEchannel_memberships` | |
| Report history | `SPreport_history` | |

Remaining entries are subscribed channels (`browseId: UC…`, under
`guideSubscriptionsSectionRenderer`, including
`guideCollapsibleEntryRenderer.expandableItems[]`).

## Signed-out home — `home-replay3.response.json`, `home-minimal-replay.response.json`

Same `richGridRenderer` shell, but the only item is a sign-in nudge —
no `richItemRenderer`, no `lockupViewModel`, no `continuationItemRenderer`.

`feedNudgeRenderer` at
`contents.twoColumnBrowseResultsRenderer.tabs[].tabRenderer.content.richGridRenderer.contents[].richSectionRenderer.content.feedNudgeRenderer`

| field | value |
|---|---|
| `title.runs[0].text` | `Try searching to get started` |
| `subtitle.runs[0].text` | `Start watching videos to help us build a feed of videos you'll love.` |

This is the `is_signed_out` signal: a home response containing
`feedNudgeRenderer` with that title (and zero video items) is the
signed-out feed. The logged-in replays (`home-replay4`,
`home-minimal-replay2`) contain no `feedNudgeRenderer`.

## Continuation token shapes (both occur)

**Shape A — endpoint form.** The feed continuation token, in every
feed that has one:

`…continuationItemRenderer.continuationEndpoint.continuationCommand.token`

| fixture | path (shortened) | token length |
|---|---|---|
| home | `…richGridRenderer.contents[].continuationItemRenderer.…` | ~2852 |
| subscriptions | same | 204 |
| history | `…sectionListRenderer.contents[].continuationItemRenderer.…` | 120 |
| search | `…sectionListRenderer.contents[].continuationItemRenderer.…` | 814 |
| continuation (response) | `onResponseReceivedActions[].appendContinuationItemsAction.continuationItems[].continuationItemRenderer.…` | 4028 |
| watch-next (Up Next) | `…secondaryResults.results[].itemSectionRenderer.contents[].continuationItemRenderer.…` | 1768 |
| watch-next (comments) | `…results.results.contents[].itemSectionRenderer.contents[].continuationItemRenderer.…` | 78 |

**Shape B — button form.** Only in watch-next Up Next, in the *same*
`continuationItemRenderer` node as shape A, carrying the *same* token
value (len 1768):

`…secondaryResults.results[].itemSectionRenderer.contents[].continuationItemRenderer.button.buttonRenderer.command.continuationCommand.token`

A parser should read shape A first and fall back to shape B.

**Not feed continuations** (do not use as the page token):

- Chip filter tokens:
  `…chipCloudChipRenderer.navigationEndpoint.continuationCommand.token`
  (home n=15, search n=6, watch-next related chips n=4).
- Chat tokens (`kta-…` prefix) under `chatInputViewModel.onWelcomeCommand
  / sendUserQueryCommand … innertubeCommand.continuationCommand.token`
  in watch-next engagement panels.
- Engagement-panel comment sort tokens under
  `sortFilterSubMenuRenderer.subMenuItems[].serviceEndpoint.continuationCommand.token`.

**Request side:** `continuation.request.json` sends the token as a
top-level string field, `{context, continuation: "<token>"}`, with no
`browseId`.

## Player — `player.response.json`, `player2`, `player3`

Not feeds; listed only so nothing is mistaken for a feed renderer:
`playerCaptionsTracklistRenderer`, `playerMicroformatRenderer`,
`playerStoryboardSpecRenderer`, `miniplayerRenderer`,
`offlineabilityRenderer`, `adPlacementRenderer`. Top-level keys:
`responseContext`, `playabilityStatus`, `streamingData`,
`playbackTracking`, `captions`, `videoDetails`, `playerConfig`,
`storyboards`, `microformat`, `trackingParams`,
`adBreakHeartbeatParams`, `frameworkUpdates`.

## `resolve_url` — `resolve_url.response.json`

Top-level keys: `responseContext`, `endpoint`. No renderers.

## Feeds with NO fixture

- **Playlist (`VL…` browse)** — dropped by the user 2026-11-16; no
  fixture exists. `playlist(id)` has no response shape to parse against.
- **Library playlists / Watch Later as browses** — browseIds are known
  from the guide (`FEplaylist_aggregation`, `VLWL`, `VLLL`), but no
  response fixture exists for those browses.
  `library_playlists()` / `watch_later()` therefore have no observed
  response shape; presumably the playlist-items generation, unverified.
- **Channel Videos tab (loaded)** — request param known
  (`EgZ2aWRlb3PyBgQKAjoA`, identical in both channel fixtures), response
  shape unobserved.
- **Search suggestions** — no endpoint in any fixture, request file, or
  HAR. The only `suggest`-matching strings anywhere are the button style
  `STYLE_SUGGESTIVE` and the hint-placement enum `suggestedPosition`.
  `search_suggestions(prefix)` has nothing to build on; capture a
  `searchbox`/`get_search_suggestions` request first.
- **RSS subscription fallback** — not an InnerTube fixture; the feed is
  `https://www.youtube.com/feeds/videos.xml?channel_id=<id>`, no saved
  XML sample exists yet.

## Chrome renderers (present in almost every fixture, not feed content)

`desktopTopbarRenderer`, `topbarLogoRenderer`,
`topbarMenuButtonRenderer`, `notificationTopbarButtonRenderer`,
`fusionSearchboxRenderer`, `hotkeyDialogRenderer`,
`hotkeyDialogSectionRenderer`, `hotkeyDialogSectionOptionRenderer`,
`multiPageMenuRenderer`, `multiPageMenuSectionRenderer`,
`compactLinkRenderer`, `voiceSearchDialogRenderer`, `buttonRenderer`,
`menuRenderer`, `menuServiceItemRenderer`,
`notificationMultiActionRenderer`, `notificationTextRenderer`,
`unifiedSharePanelRenderer`, `twoColumnBrowseResultsRenderer`,
`tabRenderer`, `aboutThisAdRenderer`, `inFeedAdLayoutRenderer`,
`adSlotRenderer`. Feed parsers can ignore these.
