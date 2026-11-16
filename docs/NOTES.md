# NOTES.md

Tiny experiments and "do not invent API behaviour" log.

- 2026-11-08 — Verified by web search: Tauri 2.11.x is the current 2.x line;
  2.11.1 is the floor for CVE-2026-42184 (GHSA-7gmj-67g7-phm9, `is_local_url`
  origin confusion, CVSS 8.8, fixed in 2.11.1). npm `@tauri-apps/api` tops
  out at 2.11.1, so Rust and JS minors must match. Pinned `tauri = "=2.11.1"`.
- 2026-11-08 — libmpv2 6.0.0 (2026-05-12, LGPL-2.1), requires libmpv 2.0 /
  mpv >= 0.35, Rust edition 2024. `libmpv2-sys 4.0.1`.
- 2026-11-08 — Svelte 5.57.0 / SvelteKit 2.70.3 / adapter-static 3.0.10 are
  the stable baseline (Sep 2026 refresh). SvelteKit 3 is RC only — not used.
- 2026-11-08 — This VM: Ubuntu 24.04, Rust 1.99.1, Node v24.22.1, no libmpv,
  no WebView2. Hence the mpv backend is feature-gated and Phase 0 is
  specified, not claimed.
- 2026-11-08 — `cargo test` results: recorded in the phase report message;
  see `cargo test --all` output. No live network in default tests.

- 2026-11-08 — Pure crates: `cargo test -p yt-model -p yt-source -p player -p store`
  = 11 tests passed, 0 failed (yt-model 2, yt-source 1 unit + 1 contract,
  player 5, store 2). `cargo fmt --check` clean, `cargo clippy --all-targets
  -- -D warnings` clean for those four crates (after fixing RepeatMode Default,
  Queue::new borrow, renaming Queue::next -> advance, and allowing the
  async-trait double-must-use lint with a comment).
- 2026-11-08 — UI: `npm install` (74 packages) + `npm run build` OK
  (built in ~6 s, static output in `ui/build`), `npm run check`
  (svelte-check) 0 errors, 0 warnings. Fixed: Vite 8 is incompatible with
  @tailwindcss/vite 4.1.12 (peer vite ^5.2 || ^6 || ^7) — pinned Vite 6.3.6
  + vite-plugin-svelte 5.1.1 for the SvelteKit 2 line; dynamic [id] routes
  need `prerender.handleUnseenRoutes: 'ignore'` with adapter-static fallback.
- 2026-11-08 — Version corrections (crates.io / npm APIs, not search):
  Tauri stable 2.12.5 (3.0.0-alpha.6 is max_version, do NOT use),
  tauri-build 2.7.4, reqwest stable 0.13.5 (staying on 0.12.28 per spec),
  rusqlite 0.41.1, libmpv2 stable 7.0.1 (scaffold keeps 6.0.0 until Phase 2
  verifies 7.x render API), SvelteKit latest 3.1.1, Svelte 5.57.3,
  adapter-static 4.0.1, @tauri-apps/api 2.12.4, serde 1.0.229, tokio 1.54.1.
- 2026-11-08 — `cargo test --all` (incl. src-tauri) FAILED to build on Linux:
  `gdk-sys` needs system `gdk-3.0` / WebKitGTK dev packages, not installed.
  apt install of libgtk-3-dev + libwebkit2gtk-4.1-dev was started; see
  PLAN §7 for outcome. This is a Linux-only build dep — Windows uses WebView2.

- 2026-11-08 — apt install of GTK/WebKitGTK dev packages timed out (>6 min,
  `apt-get update` hung, killed). So `cargo test --all` incl. src-tauri is
  NOT green on this VM: Tauri on Linux needs gdk-3.0 + webkit2gtk-4.1 system
  libs. Pure crates (11 tests) + UI build/check are green. Full Tauri build
  must be verified on a machine with those deps (Linux) or WebView2 (Windows).
  Recorded as a deviation in PLAN section 9, not hidden.

- 2026-11-08 — libmpv2 7.x verified from crate source + GitHub releases
  (not search): 7.0.1 released 2026-11-07 (ARM bugfixes only), 7.0.0
  2026-10-30, 6.0.1 2026-10-29. 7.0.0 breaking changes: property with
  format MPV_FORMAT_NONE now returns None instead of panicking; deprecated
  events are now actually disabled by `disable_deprecated_events()`.
  6.0.0 breaking: `RenderContext::new()` replaced by
  `Mpv::create_render_context()`, `'static` bound on OpenGLInitParams.
  Player pin upgraded 6.0.0 -> =7.0.1. `wait_event` returns
  `Option<Result<Event>>` — the previous backend had the signature wrong
  and would not have compiled; fixed in `mpv_backend.rs` with
  `disable_deprecated_events()` + `time_pos()` returning Option.
  The mpv backend itself still has NOT been compiled (needs system libmpv).
- 2026-11-08 — Phase 2 pure work: `crates/player/src/playback.rs`
  (speed 0.25–3x clamp, seek clamp, quality switch preserves position/
  pause/volume/speed/subtitle, audio-only toggle, chapter nav, resume
  rules, URL-failure classification). Pure tests now 17 total (player 11).
- 2026-11-08 — apt: `apt-get update` was NOT actually hung — it was
  downloading the large noble Packages files from azure.archive (the
  cogentco mirror lines are Ignored). A 300 s update was started to let
  it finish; `libgtk-3-dev` etc. were unavailable only because the
  package lists had never completed. Outcome recorded in PLAN §9.

- 2026-11-08 — Phase 3 pure transport: `crates/yt-source/src/innertube.rs`
  (request envelope, `/player` + `/search` bodies, SAPISIDHASH with a
  known test vector, FallbackChain, FailureMemory 30 min TTL + 5 min
  self-heal rate limit, revalidation rules). sha1 =0.11.0; note its
  digest type has no LowerHex impl in 0.11, so hex is encoded manually.
  No live calls, no cipher, no PoToken — those are documented Phase 3
  risks, not implemented here. Pure tests now 22 total.

- 2026-11-08 — `cargo test --all` is now GREEN (24 tests, 0 failed:
  player 11, yt-source 6 + 1 contract, yt-model 2, store 2, tubelite 2).
  `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean.
- 2026-11-08 — apt unblocked: the cogentco mirror in ubuntu.sources was
  the stall (connection failed via proxy, update never completed). Dropped
  it to azure-only; `apt-get update` then finished in ~9 s. The remaining
  blocker was libzstd-dev: it pins libzstd1 (= 1.5.5+dfsg2-2build1.1) but
  the image ships libzstd1 1.5.5+dfsg2-2build1.1+hatch1 — a SECURITY
  backport (S727832: clamp external dictionary history, init literal
  buffer padding). Downgrading was NOT done; instead libzstd-dev was
  repacked locally with the pin relaxed to `>= 1.5.5` and installed, so
  the patched runtime lib stays. /etc/apt/sources.list.d/ubuntu.sources
  backed up to /tmp/ubuntu.sources.bak; the patched .deb is
  /tmp/libzstd-dev_1.5.5+hatch-compat_amd64.deb.
- 2026-11-08 — System deps installed: libgtk-3-dev 3.24.41,
  libwebkit2gtk-4.1-dev 2.52.6, libmpv-dev (libmpv 0.37.0, client API
  2.2.0) + libayatana-appindicator3-dev, librsvg2-dev, patchelf.
- 2026-11-08 — Scaffold bug fixed: `generate_context!()` panicked on
  missing `src-tauri/icons/32x32.png`, then on RGB (not RGBA) icons.
  Placeholder RGBA icons generated (solid #ff0033) and bundle.icon
  trimmed to existing files; unused `AppHandle`/`Manager` imports removed.
  Icons are placeholders until the app name / branding is chosen.
- 2026-11-08 — mpv backend COMPILES: `cargo check -p player --features mpv`
  and `cargo test -p player --features mpv` pass against libmpv2 7.0.1 +
  system libmpv 0.37. The 7.x API fix (wait_event Option<Result<Event>>,
  disable_deprecated_events) is verified by compilation, not just reading.

- 2026-11-08 — Phase 2 scripted playback test PASSES headless
  (`crates/player/tests/mpv_playback.rs`, `--features mpv`, 7.8 s):
  fixture `tests/fixtures/test-60s.mp4` (723 KB, 60 s testsrc + 440 Hz
  sine, ffmpeg-generated, committed). Verified against real libmpv 0.37
  + libmpv2 7.0.1: position advances ~2 s in 2 s wall time, pause freezes
  position, resume continues from the pause point, 20 deterministic
  pseudo-random seeks (LCG seed 42) each settle within ±1.5 s, avsync
  stays < 0.5 s after the seeks, 2x speed advances ~4 s in 2 s, and the
  audio-only toggle works without stalling playback. New backend methods
  `set_pause`, `set_option`, `duration`, `avsync`; `MpvPlayer` re-exported
  from the player crate root. Caveat: `vo=null`/`ao=null`, so this is a
  plumbing + seek-accuracy test, not a render-window A/V check — that
  still needs Phase 0's Windows spike / a GL render test.
- 2026-11-08 — Totals after the playback test: `cargo test --all`
  24 tests green; `cargo test -p player --features mpv` adds the 1
  scripted playback test (25 with mpv). Clippy `-D warnings` clean for
  both the default and the mpv feature builds; fmt clean.

- 2026-11-08 — Live InnerTube check BLOCKED by egress, not by code:
  youtube.com GET is 200 in 0.9 s, but youtubei/v1/player POST hangs
  with 0 bytes after 30 s and 60 s for both WEB and ANDROID clients.
  Recorded in KNOWN-ISSUES.md; clients.json stays placeholder until a
  normal network verifies it. No retry loop — two timeouts is enough.

- 2026-11-08 — Phase 4 UI (early): keyboard shortcuts + command palette.
  `ui/src/lib/shortcuts.ts` (pure matcher: k/Space, j/l, arrows, f/m/t,
  0-9, Ctrl/⌘ K; dispatches `tubelite:<action>` CustomEvents for the Watch
  page to consume), `ui/src/lib/commands.ts` (navigate/theme/audio-only
  commands, pure filter), `CommandPalette.svelte` (Ctrl/⌘ K, ↑↓/Enter/Esc,
  wired into +layout). Added missing `--border` / `--accent-soft` theme
  tokens to app.css (dark + light). svelte-check 0 errors, build OK.

- 2026-11-08 — Phase 3 `playable()` parsing landed:
  `crates/yt-source/src/innertube_source.rs` — pure
  `parse_player_response` + `InnerTubeSource` (fallback chain + failure
  memory + 403/timeout retry to next client). Fixtures
  `tests/fixtures/player.json` (DASH 248/247/243, 137/136, 251/140,
  progressive 18/22, captions, expiresInSeconds 21600),
  `player_unplayable.json` (LOGIN_REQUIRED), `player_cipher.json`
  (ciphered 248, plain 251). Selection: VP9 preferred at equal height,
  AV1 excluded, Opus 251 preferred for audio, expiry = now + 21600 s.
  yt-source tests 6 -> 13 unit (+1 contract). Live call still untested
  (egress hangs on the POST — see KNOWN-ISSUES.md); search/video/channel/
  playlist parsers still return Unsupported.

- 2026-11-08 — Phase 3 search() + video() landed:
  `crates/yt-source/src/innertube_parse.rs` — pure parsers for raw
  /search (videoRenderer -> VideoCard, channel/playlist/ad renderers
  skipped, continuation token extracted) and /player -> VideoDetails
  (captions incl. auto-generated flag). Raw fixture
  `tests/fixtures/search_raw.json` (2 videos incl. a live one, channel +
  playlist + ad renderers, continuation token). `InnerTubeSource` now
  implements search/video via a shared `post_json` fallback helper;
  channel/playlist still return Unsupported. Gaps documented in code:
  chapters need the markersMap parser, like counts / publish dates /
  Up Next come from /next, not /player. yt-source tests 13 -> 17 unit.

- 2026-11-08 — Phase 3 channel() + playlist() landed:
  `innertube_parse.rs` gains `parse_channel_response` (c4TabbedHeader +
  gridRenderer videos, shelf renderers skipped) and
  `parse_playlist_response` (playlistVideoRenderer items + continuation
  token), with raw fixtures `channel_raw.json` / `playlist_raw.json`.
  `InnerTubeSource` now implements every `StreamSource` method. Also
  fixed `parse_count` ("1.2M" no longer parses as 1 — abbreviated counts
  are None) and the avatar thumbnail shape (avatar carries `thumbnails`
  directly, not nested under `thumbnail`). yt-source tests 17 -> 20 unit.

- 2026-11-08 — Phase 6 ExtractorSource landed (`crates/yt-source/src/extractor.rs`):
  fixed arg allow-list, video-id validation `^[A-Za-z0-9_-]{11}$`
  (shell metachars / URLs / wrong lengths rejected before any process
  runs), no shell (tokio::process::Command argv), 30 s timeout, 8 MiB
  stdout cap, strict single-JSON parse, expiry from the URL `expire=`
  parameter, format selection reuses the InnerTube selectors. Tests use
  fake sidecar scripts (`tests/fixtures/fake-ytdlp*.sh`) end to end:
  success, non-zero exit -> Sidecar error, garbage -> Parse error.
  yt-dlp itself is NOT installed on this Linux VM, so the real binary is
  unverified here — that check belongs on the Windows machine. Added
  tokio "process" feature to the workspace pin (still =1.54.1).
  yt-source tests 20 -> 28 unit.

- 2026-11-08 — Phase 4 UI: real Home + Settings pages.
  Home (`ui/src/routes/+page.svelte`): search box wired to the IPC
  `search_videos`, results cached per query (pageCache, 5 min / 80 pages)
  and rendered windowed via `windowedSlice` — fixed 96 px rows so the
  math is honest, with a live "rows in the DOM" note. Settings
  (`ui/src/routes/settings/+page.svelte`): quality, audio-only, theme,
  yt-dlp path, persisted through the Tauri get/set_setting IPC into the
  SQLite settings table (`ui/src/lib/settings.ts`), capabilities shown
  from `get_capabilities`, plus the required unofficial/not-affiliated
  notice. svelte-check 0 errors, build OK. Note: Svelte 5 does not allow
  `bind:scrollTop` on a div — the list uses a `bind:this` ref + onscroll.

- 2026-11-08 — App integration: source chaining in `src-tauri/src/source.rs`.
  `AnySource` enum (Fixture | Live{InnerTube + optional Extractor}),
  built at startup from the `source_mode` / `ytdlp_path` settings.
  Default is fixture while live InnerTube is unverified on this VM's
  egress. `playable()` falls back to the extractor on Cipher / Forbidden /
  Network / UrlExpired only (classification in `should_fallback`, tested);
  search/video/channel/playlist never fall back (extractor is
  Unsupported there by design). All IPC commands now delegate to
  `state.source` instead of constructing FixtureSource per call. 4 new
  tests in src-tauri (mode selection, fallback classification, fixture
  search). Total `cargo test --all` 50 green.

- 2026-11-08 — Chapters parsed: `parse_chapters` walks
  `playerOverlays...decoratedPlayerBarRenderer...markersMap[].value.chapters[]`
  (HEATSEEK entries skipped, sorted by start time). player.json fixture
  now carries 3 chapters (Intro 0s, Main part 65s, Outro 180s).
- 2026-11-08 — docs/PLAN.md §9 rewritten: it still claimed Phase 1
  partial / `cargo test --all` not green. Now records the real status
  per phase (50 tests green, Phase 2 headless-done, Phase 3 offline-done
  live-unverified, Phase 4 partial, Phase 5 not started, Phase 6
  partial, Phase 0 not run) and the two open owner decisions.

- 2026-11-08 — PlayerService (`crates/player/src/service.rs`, feature
  `mpv`): binds MpvPlayer + PlaybackState + Queue into the one owner
  src-tauri will wrap with IPC. Commands clamp through the state first,
  then apply to mpv; `sync_position()` pulls mpv's clock into the state
  (4 Hz tick per the plan). Scripted test `service_playback.rs`:
  load/play/pause/resume/seek-clamp/2x/audio-only/queue-advance all
  pass headless against the 60 s fixture. Note: the fixture's real
  duration is ~60.02 s, not exactly 60.0 — the clamp test allows that.

- 2026-11-08 — Player IPC (`src-tauri/src/player_ipc.rs`): PlayerSlot
  holds PlayerService behind the `mpv` feature (OFF by default, so CI's
  `cargo test --all` on ubuntu-latest without libmpv still builds; a
  failed libmpv init degrades to Disabled with a warning, never a
  startup failure). Commands: play_video (resolves via the source chain,
  loads the DASH pair, starts playing), player_play/pause/toggle/seek/
  set_speed/set_audio_only/status/next. Watch page now has real
  controls (play/pause, seek slider, speed, audio-only, 4 Hz status
  tick) and listens to the `tubelite:seek` / `seek-percent` /
  `play-pause` shortcut events. On a build without mpv the error is
  shown, never a silent black rectangle. Tests: 52 green default
  (incl. slot-disabled test), 8/8 with `--features mpv` (incl. a
  fixture-drive test through the slot).

- 2026-11-08 — Phase 4 pages wired (no more "Skeleton" pages).
  Store: subscriptions (add/remove/list, title-ordered) + history
  (list newest-first, limit) accessors with tests. Tauri IPC:
  list_subscriptions / subscribe / unsubscribe / list_history /
  record_history_entry / save_resume_position / get_resume_position.
  UI: shared windowed `VideoList.svelte` (96 px rows, same math as
  Home); search page reads `?q=`; channel page shows header + Subscribe
  toggle; playlist page with Play all; subscriptions page with
  Unsubscribe; library page shows history with resume positions.
  Watch page records history on open, resumes a saved position when
  meaningfully mid-video (>5 s in, >10 s from the end), and saves the
  resume position on leave. Note: svelte-check forbids nested `<a>` —
  VideoList keeps the channel link outside the video link.

- 2026-11-08 — Chapters UI: Watch page lists chapters from
  `get_video` (parsed from the /player markers map), highlights the
  chapter containing the current position, and click-to-seek jumps to
  it; the current chapter title also shows next to the time readout.
- 2026-11-08 — README status rewritten (it still described Phase 1
  only); now lists what is actually done, what is unverified, and the
  two open owner decisions.

- 2026-11-08 — Captions end-to-end. `yt-source/src/captions.rs`:
  JSON3 parser (skips seg-less events) + minimal srv3 XML parser
  (`<text start dur>` scanner + entity decode, malformed input errors
  rather than guessing) + WebVTT writer + `fetch_captions` (appends
  `fmt=json3` when absent, falls back to srv3 when the body is not
  JSON). `MpvPlayer::load_subtitles` (sub-add select) /
  `disable_subtitles` (sid=no); `PlayerService` passthroughs recording
  the language in the state. IPC `player_load_captions` (resolve track
  → fetch → VTT in the cache dir → sub-add) and
  `player_clear_captions`; filenames are sanitised to
  [A-Za-z0-9_-]. Watch page lists caption tracks with an Off button.
  Note: mpv's sub-add (MPV_ERROR_COMMAND, -12) fails if no file is
  loaded yet — the service test waits for the clock first, mirroring
  real use where captions load after playback starts.

- 2026-11-08 — Volume/mute/theatre/fullscreen. The state machine had
  volume/muted all along but nothing drove them: `PlayerService`
  now applies volume/mute to mpv (`volume` / `mute` properties, also
  re-applied on load), `PlayerStatus` carries them, and IPC
  `player_set_volume` / `player_set_muted` expose them. Watch page:
  mute button + volume slider, theatre toggle (21:9 hole, chapters
  and captions hidden), fullscreen button. The F/T/M/arrow shortcuts
  now actually work — they were advertised in the hint line but
  ignored before. Limitation, noted in code: fullscreen only
  fullscreens the webview; the Phase 0 mpv child window must be moved
  with the hole when fullscreen changes.

- 2026-11-08 — Watch Later. The `watch_later` table existed since
  Phase 1 but had no accessors; migration v2 adds `title`/`channel`
  columns (ALTER with defaults, so existing rows survive) and bumps
  SCHEMA_VERSION to 2. Store: add/remove/list with tests. IPC:
  add_watch_later / remove_watch_later / list_watch_later. UI: every
  VideoList card has a "+ Watch later" toggle (saved state fetched
  once per list), and the Library page has a Watch Later section with
  Remove buttons above History.

- 2026-11-08 — Audio-only playback fixed + in-player quality
  selector. Bug: `play_video` errored "no video stream for this
  quality" whenever the preference was AudioOnly, because sources
  return `video: None` for that preference — so Settings quality
  "audio" could never play. Fixed with `urls_for_pref`: audio-only
  loads the audio stream itself as mpv's main file (a standalone
  audio stream plays fine), video modes load the DASH pair. Covered
  by 3 unit tests. Watch page now has a quality selector (Auto /
  144p–1080p / Audio only) that re-resolves at the new quality and
  seeks back to the previous position.

- 2026-11-08 — Consolidation pass: PLAN.md §9 refreshed (62 tests,
  Phase 2 includes PlayerService + volume/mute/subtitles, Phase 3
  includes captions + source chaining + the audio-only fix, Phase 4
  marked done except the SvelteKit 3 decision), docs/MEMORY.md gains
  the caption-file cache row. Full verification re-run: fmt clean,
  clippy clean (default + `--features mpv`), `cargo test --all` 62
  green, player mpv 16 green, tubelite mpv 11 green, svelte-check +
  UI build clean.

- 2026-11-08 — Store persistence. The app opened the store
  in-memory, so every setting, subscription, history entry, resume
  position and Watch Later item was lost on restart. Now the store
  opens at `<app-data-dir>/tubelite.db` inside Tauri `setup()` (the
  dir only exists once the app handle does), creating the directory
  first; a failed open fails startup loudly instead of silently
  falling back. Covered by 2 tests, incl. a reopen-persists
  regression test. (Two API facts learned from compiler errors:
  `App::path()` does not exist — it is `app.handle().path()`, and
  `path()` on AppHandle needs `tauri::Manager` in scope.)
