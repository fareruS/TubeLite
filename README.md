# TubeLite (working name)

A lightweight native desktop YouTube video client — Tauri 2 + Rust + SvelteKit + libmpv.
No Electron, no bundled Chromium. The UI never talks to YouTube; all network,
parsing and caching live in Rust, and the UI receives small, already-shaped
structs over IPC.

> This app is unofficial and not affiliated with YouTube or Google.

## Status (2026-11-08)

- **Done offline:** workspace + `yt-model`, `yt-source` (InnerTube transport,
  fallback chain, search/video/channel/playlist parsers, chapter markers,
  yt-dlp sidecar source), `player` (state machine, queue, libmpv backend,
  PlayerService), `store` (SQLite: settings, subscriptions, history,
  resume), Tauri app (IPC guard, source chaining, player IPC behind the
  `mpv` feature), SvelteKit UI (Home with windowed search + cache,
  Watch with real controls + chapters + history/resume, Search, Channel
  with Subscribe, Playlist, Subscriptions, Library, Settings).
- `cargo test --all` **54 green** offline; `cargo test -p player
  --features mpv` adds 3 scripted playback tests against real libmpv
  (headless). No live YouTube calls in the default test run.
- **Not yet built / verified:** Phase 0 Windows video spike (no Windows
  machine here), live InnerTube verification (this VM's egress hangs on
  youtubei POST — `clients.json` versions are placeholders), cipher /
  PoToken transforms, official Data API source (needs a user-supplied
  key), OS integration (tray, media keys), installer/updater (Phase 7).
- **No Windows measurements exist yet.** All budget numbers in `docs/PLAN.md`
  are targets, not claims. See `docs/NOTES.md` for what was actually measured
  (Rust version, dependency pins, test counts).
- **Open owner decisions:** project license (MIT / Apache-2.0 / GPL-3.0)
  and app name (Litube vs TubeLite, the working name).

## Quick start (Linux dev, offline tests)

```sh
# Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
export PATH="$HOME/.cargo/bin:$PATH"

cargo test --all          # offline, no network, no libmpv needed
cargo fmt --check
cargo clippy --all-targets -- -D warnings   # Phase 1 goal; see NOTES if warnings remain

cd ui && npm install && npm run build   # SvelteKit static build
npm run check                            # svelte-check
```

Full Tauri build (needs libmpv + WebView2/WebKitGTK — Windows/Linux):

```sh
cd ui && npm run build
cargo tauri build   # from src-tauri, once the Tauri CLI is installed
```

See `docs/PLAN.md` for phases, risks and open questions, and
`docs/BUILD-PLATFORMS.md` (to be completed when a Windows/Linux build is
actually run — steps not run are not documented as verified).

## License

**Not chosen yet — the owner must pick one.** See `docs/PLAN.md` §6.1.
No LICENSE file is shipped until then. This repo reimplements architectural
ideas from open-source projects (Limusic, NewPipe Extractor, yt-dlp, etc.)
and copies no code from them. `THIRD_PARTY.md` lists dependency licenses.

## Threat model (short)

- The app talks to YouTube's internal API in Phase 3; that surface is
  fragile and can break without notice. Breakage is isolated behind
  `StreamSource` + `clients.json` + fixtures, so a fix is a config update
  or a small patch, not a rewrite.
- Cookies/tokens/API keys (if the user supplies any) go in the OS credential
  store, never in SQLite or logs. Logs are redacted (see `src-tauri/src/logging.rs`).
- The yt-dlp sidecar (Phase 6) is treated as untrusted: fixed argument
  allow-list, no shell interpolation, timeouts, output size limits, strict JSON.
- Loopback servers (if built) bind to 127.0.0.1 only, with a random per-launch token.
