# TubeLite

A lightweight native desktop YouTube video client: Tauri 2 + Rust + SvelteKit + libmpv. No Electron, no embedded browser for the main UI. The UI never talks to YouTube — all network, parsing, and caching live in Rust, and the UI receives small, already-shaped structs over IPC.

TubeLite is an unofficial client, not affiliated with YouTube or Google. It talks to YouTube's internal InnerTube API using your own browser cookies; that may conflict with YouTube's Terms of Service and can break without notice.

## What works

- Signed-in feeds: home, subscriptions, history, search, channel pages, watch-next — parsed from real captured InnerTube responses (`crates/feeds`, `crates/innertube`).
- Sign-in: Google sign-in popup, or import a Netscape `cookies.txt` (Settings → Account). Cookies live in the OS keyring, with an encrypted-file fallback where no keyring exists.
- Playback (`crates/player`): yt-dlp resolves the best video/audio pair, libmpv plays it, and a failed stream re-resolves once and resumes where you left off.
- Workspace tests: 168 passing, clippy clean (`cargo test --workspace --exclude tubelite`).

## What is not proven yet

- Real playback has never run end to end (no yt-dlp or libmpv on the dev VM). The mpv adapter compiles; it has never played a video.
- The login popup has never been opened. It needs a real Windows machine — WebView2 never paints under Wine.
- The keyring path has never been round-tripped. Only the encrypted-file fallback has been exercised.
- Player stage 2 (loopback proxy), write actions (like, subscribe, Watch Later, playlists), and the native `app/` crate are not built yet — see `docs/missing-captures.md`.

## Build

### Prerequisites

| | Windows (primary target) | Linux (dev) |
| --- | --- | --- |
| Rust | stable 1.80+ via [rustup](https://rustup.rs) (MSVC toolchain — install the Visual Studio C++ build tools) | stable 1.80+ via rustup |
| Node.js | 20+ | 20+ |
| Tauri CLI | `cargo install tauri-cli --version "=2.12.4"` | same |
| WebView2 | preinstalled on Windows 11; the installer adds it if missing | — |
| System libs | — | `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libmpv-dev` |
| Playback | `yt-dlp` on PATH (e.g. `winget install yt-dlp.yt-dlp`) and a Windows libmpv build with `libmpv-2.dll` next to the exe | `libmpv-dev` (see above) |

Versions used while developing this: Rust 1.99.1, Node v24.22.1, Tauri 2.12.5, libmpv2 7.0.1.

### Steps

```sh
git clone https://github.com/fareruS/TubeLite.git
cd TubeLite

# 1. Frontend dependencies (once)
cd ui && npm install && cd ..

# 2. Dev run (hot reload; the UI dev server runs on :1420)
cd src-tauri && cargo tauri dev

# 3. Release build — the NSIS installer lands in
#    src-tauri/target/release/bundle/nsis/TubeLite_0.1.0_x64-setup.exe
cargo tauri build -- --features mpv
```

The `--features mpv` flag matters: playback is off by default so the workspace builds and tests on machines without libmpv. Without it, the app builds fine but every playback command returns an error.

`cargo tauri build` runs the UI build first (`beforeBuildCommand`, with `cwd: ../ui`), so you don't need to build the UI separately — but `npm install` in `ui/` must have run at least once.

### Checks

```sh
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace --exclude tubelite
cargo test -p player --features mpv   # needs libmpv installed
cd ui && npm run check                # svelte-check
```

`cargo check -p tubelite --tests` is the bar for the Tauri crate: its test binaries need GTK/WebKit dev libraries to link on Linux.

## First run

1. Open Settings → Account and sign in (Google popup), or import a Netscape `cookies.txt`.
2. Without an account the feeds load signed-out (empty home, "Try searching to get started").
3. Playback needs `yt-dlp` on PATH and `libmpv-2.dll` beside the exe (Windows).

## Layout

```
crates/
  innertube/   InnerTube client: session, SAPISIDHASH auth, retry + heal-on-401
  feeds/       Feed parsers (home, subs, history, search, channel, watch-next)
  accounts/    Cookie import, keyring storage, login flow, session manager
  player/      yt-dlp resolver, libmpv player, recovery, broadcast events
  yt-model/    Shared models
  yt-source/   Legacy InnerTube source (feeds are replacing it)
  store/       SQLite store
  tools/       replay + redact (fixture tooling)
src-tauri/     Tauri app (commands, login popup, settings)
ui/            SvelteKit frontend
fixtures/      Redacted captured InnerTube responses (ground truth for parsers)
docs/          PLAN.md, renderer-map.md, missing-captures.md, KNOWN-ISSUES.md
```

Parsers are built from the fixtures in `fixtures/` and must never invent an endpoint, browse ID, or JSON path. If a shape is missing, the code says what to capture (`docs/missing-captures.md`) instead of guessing.

## Threat model (short)

- The InnerTube surface is fragile and can break without notice. Breakage is isolated behind `crates/innertube` + `crates/feeds` + fixtures, so a fix is a config update or a small patch, not a rewrite.
- Cookies go in the OS credential store (or an encrypted file fallback), never in SQLite or logs. Cookie values are redacted everywhere they could be printed.
- The yt-dlp sidecar is treated as untrusted: fixed argument allow-list, no shell interpolation, 60 s timeout, stderr redacted before it reaches any error.

## License

TBD — no license has been chosen yet. Do not redistribute until one is picked (see `docs/PLAN.md` §6.1).
