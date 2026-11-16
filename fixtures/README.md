# fixtures/ — Phase 0 ground truth (real, redacted InnerTube captures)

Real responses captured from a signed-in desktop browser, saved only as
redacted copies here. Raw captures and cookies are never committed.

## Layout

- `raw/` — gitignored. Raw response JSON, saved request JSON
  (`<name>.request.json`), `cookies.txt` (Cookie header value), and
  `redact-strings.txt` (one personal string per line: your own channel ID,
  account name, etc.). Never committed, never pasted into chat.
- `redacted/` — committed. Output of `tools/redact.py`. This is what parsers
  and tests read.
- `tools/replay.py` — re-sends a saved request with a freshly computed
  SAPISIDHASH (from `raw/cookies.txt`) and saves the response to `raw/`.
- `tools/redact.py` — strips cookies, auth, visitorData, emails, signed-URL
  queries, and your `--extra` strings from a raw file.
- `tools/test_phase0.py` — offline tests (mock server, no real network).

## Capture procedure (what was done)

1. Signed in to youtube.com in a desktop browser with the dev Google account.
2. DevTools → Network, filter `youtubei`.
3. Visited home, subscriptions, history, a playlist, a search, a channel,
   and a watch page; scrolled to trigger a continuation.
4. For each, copied the matching browse / search / next request as cURL,
   saved the request (method, URL, headers, JSON body) and the response.

## Provenance

Date format: YYYY-MM-DD. "Source" is the page visited; "request" is the
InnerTube endpoint + body shape actually sent by the browser — nothing here
is guessed.

| fixture (redacted/) | source page | request | client | captured |
|---|---|---|---|---|
| `home.request.json` / `home.response.json` | Home (`/`) | `POST /youtubei/v1/browse?prettyPrint=false`, `browseId: FEwhat_to_watch` | WEB 2.20261112.10.00 | 2026-11-16 |
| `subscriptions.request.json` / `subscriptions.response.json` | Subscriptions (`/feed/subscriptions`) | `POST /youtubei/v1/browse?prettyPrint=false`, `browseId: FEsubscriptions` | WEB 2.20261112.10.00 | 2026-11-16 |
| `history.request.json` / `history.response.json` | History (`/feed/history`) | `POST /youtubei/v1/browse?prettyPrint=false`, `browseId: FEhistory` | WEB 2.20261112.10.00 | 2026-11-16 |
| `search.request.json` / `search.response.json` | Search results | `POST /youtubei/v1/search?prettyPrint=false`, `query: "rust programmming"` (as typed) | WEB 2.20261112.10.00 | 2026-11-16 |
| `channel.request.json` / `channel.response.json` | Channel page (opened from Subscriptions) | `POST /youtubei/v1/browse?prettyPrint=false`, `browseId: UC…` (channel id) | WEB 2.20230327.07.00 | 2026-11-16 |
| `channel2.request.json` / `channel2.response.json` | Second channel page | `POST /youtubei/v1/browse?prettyPrint=false`, `browseId: UC…` | WEB 2.20230327.07.00 | 2026-11-16 |
| `player.request.json` / `player.response.json` | Watch page (video opened from Subscriptions) | `POST /youtubei/v1/player?prettyPrint=false`, `videoId` | WEB 2.20261112.10.00 | 2026-11-16 |
| `player2` / `player3` (same shape) | Two more watch pages | `POST /youtubei/v1/player?prettyPrint=false`, `videoId` | WEB 2.20261112.10.00 | 2026-11-16 |
| `watch-next.request.json` / `watch-next.response.json` | Watch page | `POST /youtubei/v1/get_watch?prettyPrint=false` (see note below) | WEB 2.20261112.10.00 | 2026-11-16 |
| `continuation.request.json` / `continuation.response.json` | Home, scrolled | `POST /youtubei/v1/browse?prettyPrint=false`, `{context, continuation}` (no browseId) | WEB 2.20261112.10.00 | 2026-11-16 |
| `guide.request.json` / `guide.response.json` | History page (`/feed/history`) | `POST /youtubei/v1/guide?prettyPrint=false` | WEB 2.20261112.10.00 | 2026-11-16 |
| `resolve_url.request.json` / `resolve_url.response.json` | Channel page navigation | `POST /youtubei/v1/navigation/resolve_url?prettyPrint=false` | WEB 2.20230327.07.00 | 2026-11-16 |

Playlist was dropped by the user (2026-11-16) — the fixture set is complete
at 7 pages plus the guide/resolve_url extras. The three HARs in `raw/`
(`homehar.har`, `subcription.har`, `history.har`, plus the all-in-one
`all.har`, all gitignored) also contained no cookies — Chrome's HAR export
carried no Cookie headers, so `raw/cookies.txt` still does not exist and the
replay check is blocked on a separate cookie export.

## Observed request shape (2026-11-16, ground truth — not guessed)

- Base: `POST https://www.youtube.com/youtubei/v1/<endpoint>?prettyPrint=false`.
- Client: `WEB 2.20261112.10.00` for browse (home), player, and guide.
  `WEB 2.20230327.07.00` (older) appears on `navigation/resolve_url` and on
  the channel browse, whose request body carries only
  `{clientName, clientVersion}` — a minimal client context from a different
  page code path. The placeholder `2.20250101.00.00` in
  `crates/yt-source/clients.json` is now known-stale.
- Home browse body: `{context, browseId: "FEwhat_to_watch", inlineSettingStatus}`.
- Subscriptions browse body: `{context, browseId: "FEsubscriptions"}`.
- History browse body: `{context, browseId: "FEhistory"}`.
- Search body: `{context, query, inlineSettingStatus, webSearchboxStatsUrl}`.
- Continuation body: `{context, continuation}` — no browseId; the token is
  a ~2.9 KB opaque string.
- Channel browse body: `{context, browseId: "<channel id>", param}`.
- Player body: `{context, videoId, playbackContext, racyCheckOk,
  contentCheckOk, params, serviceIntegrityDimensions}`.
- Watch-next: the watch page does **not** call `/next` on this client —
  it calls `POST /youtubei/v1/get_watch?prettyPrint=false` with
  `{context, playerRequest, watchNextRequest}`, and the response is a
  JSON array of two items: `[0].playerResponse` (same shape as `/player`)
  and `[1].watchNextResponse` (`contents, currentVideoEndpoint,
  engagementPanels, …`).
- Guide body: `{context, fetchLiveState}`. Resolve_url body: `{context, url}`.
- Headers sent (names only): `origin`, `referer`, `user-agent`,
  `content-type: application/json`, `x-youtube-client-name`,
  `x-youtube-client-version`, `x-youtube-bootstrap-logged-in`,
  `x-goog-authuser`, `x-origin`, plus `sec-ch-*` / `sec-fetch-*` fetch
  metadata. Cookie/Authorization values are never stored in request files;
  the replay tool injects them from `raw/cookies.txt`.
- Response top-level keys — home browse: `contents, frameworkUpdates,
  header, onResponseReceivedActions, responseContext, topbar,
  trackingParams`; player: `adBreakHeartbeatParams, captions,
  frameworkUpdates, microformat, playabilityStatus, playbackTracking,
  playerConfig, responseContext, storyboards, streamingData,
  trackingParams, videoDetails`; guide: `items, responseContext,
  trackingParams, userProfile`.

Existing synthetic fixtures in `crates/yt-source/tests/fixtures/` are NOT
ground truth — they are hand-made shapes for offline tests. They stay
separate until replaced by redacted captures from this directory.

## Rules

- Never commit `raw/` or `cookies.txt`. `.gitignore` covers `fixtures/raw/`
  and `**/cookies.txt`.
- Never print a cookie, SAPISID, or SAPISIDHASH value. `replay.py --dry-run`
  prints header names only.
- A fixture is done only when `redact.py` verification passes and the
  replayed home request returns a logged-in feed (checked against a
  signed-out replay, not by eyeballing one response).

## Done criteria (Phase 0)

- [x] Replaying the home request returns a logged-in feed. Verified
      2026-11-16: with cookies from the SAME account as the HAR capture,
      both the full captured request and the minimal-body request came back
      logged in — full: 1,091,522 bytes, 29 grid items, 45 videos, 20
      avatars, no sign-in prompt; minimal: 844,999 bytes, 27 items, 43
      videos, 17 avatars. (Earlier failures were all with the other
      account's cookies: account A's session data + account B's cookies =
      signed out.) Evidence in `redacted/home-replay4.response.json` and
      `redacted/home-minimal-replay2.response.json`. The VM itself still
      cannot run the replay (egress hangs on youtubei POSTs, see
      `docs/KNOWN-ISSUES.md`) — verification was done on the user's
      machine via `raw/replay-home.zip`.
- [x] One redacted fixture each: home, subscriptions, history, search,
      channel, watch-next (`get_watch`), continuation (done 2026-11-16).
- Playlist: dropped, user doesn't need it (2026-11-16).
- [x] This README records source + date for each fixture.
