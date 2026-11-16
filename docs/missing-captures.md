# Missing captures

Work that is blocked because the request/response shape is not in
the fixtures yet. Nothing here is built until the capture exists —
no endpoint name, browse ID, or JSON path is guessed.

How to capture (same as Phase 0): in Chrome, open DevTools →
Network, filter `youtubei`, do the action, right-click the matching
request → Save. Save the request (method, URL, headers, body) and
the response as a pair under `fixtures/` (`<name>.request.json` +
`<name>.response.json`), then run them through the redactor before
they land in `fixtures/redacted/`. The cookie export (`cookies.txt`)
is never committed.

## 1. Account menu — needed by `crates/accounts`

`validate(cookie)`, `start_keepalive`, `list_identities`, and
`select_identity` all need the request that returns the signed-in
account (name, handle, thumbnail, channel ID, and the brand-account
identities). No fixture contains it today: no request file mentions
`account`, and `dataSyncId` / `onBehalfOfUser` appear nowhere in
`fixtures/redacted/`.

Capture:
1. Sign in on youtube.com, open DevTools → Network, filter
   `youtubei`.
2. Click the avatar (top-right) to open the account menu.
3. Find the request whose response contains the account menu (your
   name, handle, thumbnail). Save it as
   `fixtures/redacted/account-menu.request.json` +
   `account-menu.response.json`.
4. For brand accounts: in the same menu click "Switch account",
   save the request that lists the identities as
   `account-identities.request.json` +
   `account-identities.response.json`, and the request that fires
   when you pick one identity as
   `account-select.request.json` + `account-select.response.json`.

Until these exist, `validate` / keepalive / brand-account switching
are not implemented (the storage, parsing, and keyring work in
`crates/accounts` does not need them and goes ahead).

## 2. Write actions — needed by `crates/innertube` writes

`fixtures/writes/` does not exist yet. Each action below becomes a
typed function only after its fixture is recorded. Record only the
actions you want; anything unrecorded stays unimplemented.

For each action: do it once in the browser with DevTools → Network,
filter `youtubei`, and save the request + response pair as
`fixtures/writes/<action>.request.json` +
`fixtures/writes/<action>.response.json`.

| Action | Browser steps |
|---|---|
| Like a video | Open a video, click Like (thumbs up). |
| Remove a like / dislike | Click the active Like again, then Dislike. |
| Subscribe | Open a channel, click Subscribe. |
| Unsubscribe | Click Subscribed → Unsubscribe. |
| Add to Watch Later | Hover a video → ⋮ → Save to Watch Later. |
| Remove from Watch Later | Same, uncheck it (or from the Watch Later page). |
| Create a playlist | Library → New playlist, give it a title. |
| Add to a playlist | Hover a video → ⋮ → Save to playlist → pick one. |
| Remove from a playlist | On the playlist page, ⋮ on the video → Remove. |

## 3. `app/` UI framework — needed by the `app/` crate

The spec says "egui or iced; I will tell you which". No UI code is
written until the framework is chosen.
