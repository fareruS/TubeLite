# PLAN — TubeLite (working name, replace `[NAMA_APP]`)

Lightweight native desktop YouTube video client. Tauri 2 + Rust + SvelteKit + libmpv.
Video counterpart of the Limusic-style lean music client. No Electron, no bundled Chromium.

Date: 2026-11-08. Environment for this build session: Ubuntu 24.04 VM,
Rust 1.99.1 (installed 2026-11-08 via rustup, stable), Node v24.22.1, no libmpv
installed, no Windows machine available in this session.
That constrains what can be measured here — see § Measurement / deviations.

## 0. Decisions already made (from the brief)

- Shell: Tauri 2, Rust stable, Cargo workspace, several crates.
- Playback: libmpv via maintained `libmpv2` crate (render API / child window).
- UI: SvelteKit SPA, adapter-static, Svelte 5 runes, TypeScript, Tailwind 4.
- Storage: SQLite (rusqlite, bundled), WAL, synchronous=NORMAL.
- HTTP: one shared reqwest client, rustls-tls, default-features=false.
- Async: tokio, bounded worker threads.
- Logging: tracing with redaction. Never log/commit cookies, tokens, API keys, signed URLs.
- UI never talks to YouTube. All network/parsing/caching in Rust. UI gets small shaped structs over IPC.
- StreamSource abstraction isolates everything fragile; client identities in
  `clients.json` + on-disk override, fallback chain, per-video failure memory,
  URL validation + expiry, fixtures for every parsed shape.
- Unofficial / not affiliated notice: once in README and once in Settings. No warnings
  scattered through code.

## 1. Pinned versions verified 2026-11-08 (do not float)

Sources checked by web search this session; re-verify against docs.rs / crates.io
at each phase bump and record the resolved version in Cargo.lock.

| Dependency | Pin used in this repo | Evidence |
|---|---|---|
| Rust | stable 1.99.1 (this VM) | `rustc --version` |
| Tauri (Rust) | `=2.12.5` | crates.io stable 2026-11-08 (verified via crates.io API, not search). 2.11.1 was the floor for CVE-2026-42184 / GHSA-7gmj-67g7-phm9 (`is_local_url` origin confusion); 2.12.5 includes that fix. Do NOT use Tauri 3 — 3.0.0-alpha.6 exists and must not be picked up by a bare `2` requirement. |
| tauri-build | `=2.7.4` | crates.io stable 2026-11-08 |
| @tauri-apps/api (JS) | `2.12.4` | npm registry latest 2026-11-08; same minor (2.12) as Rust crate, as Tauri requires |
| libmpv2 | `=7.0.1` (upgraded 2026-11-08) | crates.io stable, released 2026-11-07. 7.0.0 breaking changes verified from GitHub releases + crate source: MPV_FORMAT_NONE -> None (was panic), `disable_deprecated_events()` now actually works. `wait_event` returns `Option<Result<Event>>`. Render API is `Mpv::create_render_context()` (since 6.0.0). Requires libmpv 2.0 / mpv >= 0.35. |
| mpv (native) | baseline to pin in Phase 2 | mpv 0.42.0 released 2026-11-05; check whether a 0.42-based mpv-dev build exists, else pin newest available 0.41-based build in `docs/BUILD-PLATFORMS.md` |
| Svelte | `5.57.0` in UI scaffold; **5.57.3 is latest** (npm, 2026-10-26) | — |
| SvelteKit | `2.70.3` in UI scaffold; **3.1.1 is latest stable** (npm, 2026-11-08) | SvelteKit 3 released Oct 2026 — it is NOT RC any more (earlier search was stale). Decision deferred: stay on 2.70.3 for Phase 1 (known Tauri adapter-static SPA setup), migrate to 3.x in Phase 4 after verifying the SvelteKit 3 adapter-static SPA config. Do not copy 2.x config blindly into 3.x. |
| @sveltejs/adapter-static | `3.0.10` (pairs with SvelteKit 2.x); 4.0.1 pairs with SvelteKit 3.x | npm latest 4.0.1 |
| reqwest | `=0.12.28` (last 0.12), default-features=false, rustls-tls, gzip, brotli, stream, json | Spec pins 0.12. 0.13.5 is the new major line (Jul 2026) — revisit in Phase 3, check rustls/feature-flag changes first. |
| rusqlite | `=0.41.1` (bundled SQLite) | crates.io stable 2026-10-28 |
| tokio | `=1.54.1`, minimal features | crates.io stable 2026-11-08 |
| serde | `=1.0.229` | crates.io stable 2026-11-08 |

Correction log (2026-11-08): the first pin table in this file used stale
search results (Tauri 2.11.x, SvelteKit 3 RC, rusqlite 0.32). The groundwork
file `~/workspace/youtube-client/PHASE0-GROUNDWORK.md` flagged the drift, and
the pins above were re-verified directly against crates.io / npm registries.
Also from groundwork: Limusic is GPL-3.0 (verified live, commit d1937ecc,
v1.4.0-rc.2, 2026-10-27); its README says it runs player-script JS in a real
hidden webview, which this spec explicitly forbids — the embedded, isolated,
destroyable JS runtime is a Phase 3 risk to spike early, not a detail to
discover in Phase 3. RustyPipe is stale (0.11.4, 2026-04-23) — treat as a
reference to read, not a maintained dependency to pin, unless a newer
release / active fork is found at Phase 3.

TRADEOFF: `player` is feature-gated. `cargo test` / `cargo build` by default
builds `player` WITHOUT libmpv (`--no-default-features` path), so the pure
crates stay testable on machines without libmpv installed (this VM). Enable
`player/mpv` only where libmpv is installed. The mpv code path is isolated in
`crates/player/src/mpv_backend.rs` and is not compiled in the default build.
Revisit when a Windows/Linux dev machine with libmpv is available.

## 2. Architecture

```
ui/ (SvelteKit SPA, local files only)
  -> Tauri invoke (typed, IPC guard by webview label)
    -> src-tauri: commands / orchestrator / event pump / tray / media keys
      -> crates/yt-source: StreamSource trait
           -> FixtureSource (offline, Phase 1) | InnerTubeSource (Phase 3)
           -> ExtractorSource / yt-dlp sidecar (Phase 6) | OfficialSource / Data API v3 (Phase 6)
      -> crates/player: libmpv wrapper (queue, gapless, speed, quality, subs, chapters)
      -> crates/store: SQLite (history, subs, settings, cache, resume)
      -> crates/yt-model: pure types only, no I/O
```

Boundary rules (enforced, not aspirational):
- `yt-model` and `yt-source` compile and unit-test offline against JSON
  fixtures, with no Tauri / webview / mpv / OS dependency.
- The app crate (`src-tauri`) is the only place that knows about all other crates.
- IPC is the only channel UI <-> core. Commands are registered behind a guard
  rejecting any webview whose label is not an approved UI window.
  Capabilities are scoped per window; each window gets minimum permissions.

### StreamSource trait

```rust
trait StreamSource {
    async fn search(&self, q: &SearchQuery) -> Result<Page<VideoCard>>;
    async fn video(&self, id: &VideoId) -> Result<VideoDetails>;
    async fn channel(&self, id: &ChannelId) -> Result<ChannelPage>;
    async fn playlist(&self, id: &PlaylistId, cursor: Option<Cursor>) -> Result<Page<VideoCard>>;
    async fn playable(&self, id: &VideoId, pref: QualityPref) -> Result<PlayableMedia>;
    fn capabilities(&self) -> Capabilities;
}
```

Capabilities drive the UI: a mode that cannot provide a feature hides/disables
it with a tooltip, never shows a broken button.

### Playback (libmpv) — options to set explicitly, with reasons in code

- `ytdl=no` — resolution is ours, not mpv's script hook.
- `gapless-audio=yes`, `prefetch-playlist=yes`.
- `hwdec=auto-safe`; Settings toggle forces software decode; log active decoder.
- Disk cache: `cache=yes`, `cache-on-disk=yes`, `demuxer-cache-dir` in app cache
  dir, `demuxer-max-bytes` / `demuxer-max-back-bytes` start 64 MiB each for audio,
  128/64 MiB for video — tune by measurement. These bound packet metadata, not
  audio bytes, when cache-on-disk is on.
- `cache-pause-wait=0.3` for faster resume after seek.
- DASH pair: video-only + audio-only attached to the same mpv item (one clock).
  Never two clocks reconciled in JS. Progressive itag 18/22 only as <=720p fallback.
- Quality switch: preserve position, pause, volume, speed, subtitle choice;
  target gap <500 ms, report actuals.
- Audio-only: `vid=no`; switching back restores picture at same position.
- Speed 0.25–3x with pitch correction, chapters, subtitles (auto-select by
  system language), resume, loop, 1-item lookahead.
- Loudness normalisation: optional audio filter, off by default.
- Retry at demuxer level for transient HTTP failures.
- Loopback chunking proxy: build ONLY if measurement shows throttling
  (127.0.0.1, ephemeral port, random per-launch token, ~4 MiB bounded ranges,
  registration cap, stall timeout).
- Expiring URLs: store expiry, re-resolve on expiry / 403, in-memory URL cache
  cap 8 entries, 4 h TTL.

### Video rendering

- Windows: mpv draws into a native child window beneath the (transparent)
  webview region where the picture belongs; page leaves a hole and reports its
  rectangle (physical px = display scale × page zoom) over IPC. Handle resize,
  DPI, z-order, fullscreen, multi-monitor.
- Linux: mpv render API into a GL area beneath a transparent webview, or the
  same child-window approach if simpler; verify Wayland and X11.
- macOS: stretch scope; document plan, do not block earlier phases.
- Modes: windowed, theater, fullscreen, always-on-top mini player,
  PiP-style floating window. Mini player must not construct a second full SPA
  if avoidable; measure cost of a second webview and decide with data.

## 3. Phases and acceptance criteria

- **Phase 0 — spike (this session, partial).** Tauri window + transparent
  webview; mpv playing a local file into a child window under the webview on
  Windows; rectangle sync from UI. Deliver `docs/SPIKE.md` with measurements.
  Status: NOT runnable on this Linux VM without libmpv / WebView2. Spike is
  documented as a Windows-only experiment; the scaffold below is the
  portable part. Do not claim Phase 0 done.
- **Phase 1 — skeleton (done in this session, offline).** Workspace, yt-model,
  yt-source trait + fixture-backed fake source, store with migrations, IPC
  guard, capabilities, logging with redaction, CI (fmt, test, svelte-check).
  Acceptance: `cargo test --all` green offline. ⚠️ PARTIAL (see §9):
  pure crates 11/11 green, UI build/check green, but `cargo test --all`
  incl. src-tauri is NOT green on this VM (missing GTK/WebKitGTK system deps).
- **Phase 2 — playback core.** player crate with mpv options above, queue,
  resume, quality switching, speed, subtitles, chapters. Acceptance: scripted
  playback test; A/V in sync after 20 random seeks.
- **Phase 3 — primary source.** InnerTubeSource with clients.json + override,
  fallback chain, URL validation, embedded JS runtime for player-script
  transforms, token generation through a maintained library, fixtures,
  nightly canary job. Acceptance: cold start search → open → play; fixtures
  pass offline; canary green.
- **Phase 4 — UI.** Screens, command palette, shortcuts, themes, lazy locales,
  skeletons, page cache, windowing. Acceptance: UI performance rules satisfied;
  frame-drop target met.
- **Phase 5 — OS integration / lifecycle.** Tray, media keys, taskbar buttons,
  close-to-tray with WebView2 suspend experiments, single instance, mini
  player, window state. Acceptance: hidden-to-tray memory target met or gap
  documented with data.
- **Phase 6 — backup sources.** ExtractorSource (yt-dlp sidecar, hardened
  subprocess, update button) and OfficialSource (Data API + IFrame in a
  dedicated lazily-created webview with no IPC). Acceptance: contract tests
  pass for all three sources; automatic failover shows a clear UI message.
- **Phase 7 — polish / release.** Installer, signed updater, diagnostics
  button with redaction, README with honest limitations, `docs/MEMORY.md`,
  final measurement report vs budget.

After each phase: run measurement protocol, update docs, commit with a message
stating what changed and the numbers.

## 4. Resource budget (targets — report actuals, never guess)

All for the entire process tree on Windows 11, sum of Private Bytes of app +
every child (incl. all msedgewebview2.exe), 1080p windowed.

| State | Target |
|---|---|
| Cold start to interactive home | < 1.5 s, mid laptop, SSD |
| Idle, window visible, nothing playing | < 150 MB |
| Playing 1080p, window visible | < 300 MB (video decode excluded if HW decode uses GPU mem; report both) |
| Hidden to tray, audio-only or paused | < 90 MB |
| Installer size | report; keep libmpv build minimal |
| CPU while playing, UI idle | < 3% of one core above decoder's own use |
| Dropped UI frames scrolling 500-row list | < 5% frames over 20 ms at 4x CPU throttle |

If a target is unreachable, say so with data, propose the nearest achievable
target, and continue. Do not invent numbers.

## 5. Risks — top 5

1. **InnerTube fragility (highest).** Client versions, player JS, cipher and
   PoToken change without notice. Mitigation: StreamSource isolation,
   clients.json + override, fallback chain, fixtures, canary job, KNOWN-ISSUES.md.
   This is the phase that decides if the product survives real YouTube.
2. **PoToken / BotGuard.** Needs a maintained library or extractor sidecar,
   not bespoke code here. If no maintained path works, ship no-PoToken fallback
   clients first and say so explicitly. Do NOT invent a working token flow.
3. **Windows video surface.** Child-window-under-transparent-webview is the
   hardest OS integration (resize, DPI, z-order, fullscreen, multi-monitor).
   Phase 0 spike exists to de-risk this before Phase 2/4 depend on it.
4. **libmpv distribution.** Windows: pinned baseline x86_64 build (not AVX2-only),
   import lib from DLL exports, bundle `libmpv-2.dll` (~117 MB on disk, gitignored,
   never commit), SHA-256 pinned, imports limited to system libraries.
   Linux: system libmpv2 + webkit2gtk-4.1. macOS: dylibbundler, re-sign.
5. **Memory creep via UI.** Windowing, bounded page cache (80 entries / 5 min),
   thumbnail disk cache with cap, no per-tick `<html>` writes, hover = colour
   only, rAF only while visible+changing. Every rule in `docs/UI-PERFORMANCE.md`
   with before/after numbers in the same commit that introduces the effect.

## 6. Open questions (need a human decision — do not guess)

1. **License — ESCALATE.** Choosing the project license is a listed escalation.
   No LICENSE file is committed in this session. Options the owner must pick:
   MIT, Apache-2.0, or GPL-3.0. Note: Limusic is GPL-3.0; this repo reimplements
   from architectural ideas only and copies no Limusic code, so MIT/Apache are
   available — but only the owner can choose. Until chosen, `THIRD_PARTY.md`
   lists dependency licenses, and no GPL code is copied.
2. **App name.** Working name `TubeLite` / crate `tubelite`. Replace `[NAMA_APP]`
   everywhere (Tauri identifier, SQLite filename, audio-client-name) once chosen.
3. **Target OS order.** Brief says Windows primary. This session is Linux-only,
   so Windows measurements (WebView2 suspend, child-window video, installer)
   are unverified here and must be run on Windows before Phase 5/7 claims.
4. **Accounts / API keys.** No spending, no accounts, no API keys created in
   this session. OfficialSource needs a user-supplied Data API v3 key in
   Settings (Phase 6); that is the user's key, entered by them.
5. **macOS.** Stretch scope; plan documented, not blocking earlier phases.

## 7. What was actually done in this session (2026-11-08)

- Installed Rust 1.99.1 via rustup (minimal profile).
- Verified Tauri 2.11.x / libmpv2 6.0.0 / Svelte 5.57 / SvelteKit 2.70.3 /
  adapter-static 3.0.10 by web search; pins recorded above.
- Scaffolded workspace: `crates/yt-model`, `crates/yt-source` (trait +
  `FixtureSource` + contract tests), `crates/player` (queue logic pure-Rust,
  mpv backend feature-gated), `crates/store` (SQLite migrations v1, typed
  accessors), `src-tauri` (IPC guard, capabilities, commands, logging/redaction),
  `ui/` (SvelteKit SPA skeleton, routes, windowing + page-cache libs, i18n
  en.json, themes via CSS variables).
- Measurement scripts `scripts/measure.sh` / `scripts/measure.ps1` (protocol,
  not yet run on Windows — no Windows numbers exist yet).
- Docs: `README.md` (with unofficial notice), `docs/MEMORY.md` (cache caps),
  `docs/UI-PERFORMANCE.md`, `docs/KNOWN-ISSUES.md`, `docs/NOTES.md`,
  `THIRD_PARTY.md` (dependency licenses, to be completed when LICENSE chosen).
- `cargo test` offline for the pure crates (11 tests, green); `cargo test --all`
  incl. src-tauri is NOT green on this VM (missing GTK/WebKitGTK system deps,
  see §9). No live YouTube network calls were made in the default test run.

## 8. Deviations from the brief (with reasons)

1. **Phase 0 spike not run on Windows.** No Windows machine / WebView2 /
   libmpv in this VM. Spike is specified, not claimed. Windows work is
   explicitly unverified until run there.
2. **Player mpv backend feature-gated off by default.** Required so
   `cargo test --all` passes offline on machines without libmpv. The brief's
   mpv options are implemented as the documented config in `player/src/config.rs`
   and applied only when the `mpv` feature is enabled.
3. **npm instead of pnpm for the UI scaffold in this session.** pnpm was not
   installed; npm 10.9.4 (bundled with Node 24) was used so `npm install`
   could be verified. Switch to pnpm is a one-line change; record it as a
   follow-up, not a silent substitution.
4. **No LICENSE file.** Escalation — awaiting owner choice (see §6.1).
5. **No live InnerTube / yt-dlp / Data API code in Phase 1.** Correct per
   phases: those are Phase 3 / Phase 6. `yt-source` ships the trait,
   `FixtureSource`, and the contract-test harness they must all pass.

## 9. Acceptance — actual results (updated 2026-11-08)

### Phase 1 (skeleton) — DONE
- `cargo test --all` (incl. src-tauri): **GREEN — 64 tests, 0 failed** (+5 scripted mpv tests under `--features mpv`)
  (was NOT green at first: fixed by dropping the unreachable cogentco apt
  mirror to azure-only, repacking libzstd-dev with its pin relaxed to
  `>= 1.5.5` — the image's libzstd1+hatch1 is a security backport,
  S727832, and was NOT downgraded — installing libgtk-3-dev 3.24.41,
  libwebkit2gtk-4.1-dev 2.52.6, libmpv-dev 0.37, and fixing missing/RGB
  placeholder icons). `cargo clippy --all-targets -- -D warnings` clean,
  `cargo fmt --check` clean. UI: `npm run build` OK, `npm run check`
  0 errors / 0 warnings.

### Phase 2 (playback core) — DONE (headless)
- Pure playback state machine + scripted playback tests against real mpv
  (libmpv2 7.0.1, libmpv 0.37): play advances, pause freezes, resume
  continues, 20 deterministic random seeks settle ±1.5 s, avsync < 0.5 s,
  2x speed, audio-only toggle, volume/mute, subtitle load/clear.
  `PlayerService` binds MpvPlayer + PlaybackState + Queue; src-tauri
  wraps it with IPC behind the `mpv` feature (off by default so CI
  without libmpv still builds). Headless (`vo=null`/`ao=null`) — the
  render-window A/V check still needs Phase 0's Windows spike.

### Phase 3 (InnerTube) — IMPLEMENTED OFFLINE, LIVE UNVERIFIED
- Transport, SAPISIDHASH, fallback chain, 30-min failure memory;
  `playable()` (DASH pair, VP9 preferred, AV1 excluded, Opus 251),
  `search()` / `video()` / `channel()` / `playlist()` parsers, chapter
  markers, caption fetching (JSON3 + srv3 → WebVTT) — all tested on
  fixtures (see `docs/NOTES.md`). Sources chained in the app crate
  (Fixture default; InnerTube + yt-dlp fallback by setting).
- **Live check BLOCKED**: this VM's egress hangs on youtubei/v1/player
  POST (GET works), so `clients.json` versions remain unverified
  placeholders. Cipher transform / PoToken NOT implemented (documented
  risks: embedded JS runtime not chosen, RustyPipe stale).
- Audio-only playback bug found and fixed in this session:
  `play_video` errored "no video stream" for the AudioOnly preference;
  it now loads the audio stream as mpv's main file.

### Phase 4 (UI) — DONE except SvelteKit 3 decision
- Done: keyboard shortcuts + command palette, real Home (windowed search
  list + page cache), Settings (quality/audio-only/theme/yt-dlp path via
  Tauri settings IPC + capabilities + unofficial notice), Watch page
  (real controls, 4 Hz tick, in-player quality selector, chapters,
  captions, theatre/fullscreen, volume/mute, history + resume),
  Search / Channel (Subscribe toggle) / Playlist (Play all) /
  Subscriptions / Library (Watch Later + History) pages, shared
  windowed `VideoList`, Watch Later (store migration v2).
- Store now persistent: opened at `<app-data-dir>/tubelite.db` in
  `setup()` (settings, subscriptions, history, resume, Watch Later
  survive restarts — previously all in-memory and lost); a failed
  open fails startup loudly, never silently falls back.
- Open: SvelteKit 3 migration decision (deferred, scaffold stays on
  2.70.3); Up Next / like counts / publish dates need `/next` parsing
  (not started — response shape unverified without a live call).

### Phase 5 (OS integration) — NOT STARTED
- Tray, media keys, WebView2 suspend experiments — Windows work.

### Phase 6 (backup sources) — PARTIAL
- ExtractorSource (yt-dlp sidecar) done and tested with fake sidecars;
  real yt-dlp not installed on this VM (unverified). OfficialSource
  (Data API v3) not started — needs a user-supplied API key.

### Phase 0 (Windows spike) — NOT RUN
- Needs a Windows 11 machine with WebView2 + libmpv DLL. No Windows
  measurements exist anywhere in this repo; all budget numbers are
  targets, not claims.

### Open decisions (owner)
- License (MIT / Apache-2.0 / GPL-3.0) and app name (Litube vs TubeLite)
  — still unanswered; blocks LICENSE file and final naming.
