# KNOWN-ISSUES.md

Log each breakage and its fix here, with dates. Every response shape parsed
is recorded as a JSON fixture.

## Open

- 2026-11-08 — Phase 0 Windows spike not run. No WebView2 / libmpv / child-window
  video on this Linux VM. Windows video surface, resize/DPI/z-order/fullscreen
  are unverified.
- 2026-11-08 — Client versions in `crates/yt-source/clients.json` are
  placeholders (WEB 2.20250101.00.00, ANDROID 19.09.37, IOS 19.09.3).
  They are NOT verified working. Phase 3 must verify each against live
  YouTube and record the verified version + date here.
- 2026-11-08 — No LICENSE chosen (owner decision, see PLAN §6.1).
- 2026-11-08 — `player` mpv backend is feature-gated off by default; no
  real playback has been tested in this repo yet.
## Fixed

- 2026-11-08 — Store now opens at `<app-data-dir>/tubelite.db`
  (created in `setup()`, failed open fails startup loudly). Settings,
  subscriptions, history, resume positions and Watch Later persist
  across restarts; previously everything was in-memory and lost.



## Live InnerTube verification blocked on this VM's egress (2026-11-08)
- `GET https://www.youtube.com/` returns 200 in ~0.9 s through the egress
  proxy, but `POST https://www.youtube.com/youtubei/v1/player` (WEB and
  ANDROID clients, no auth, no cookies) times out with 0 bytes received
  after 30 s and again after 60 s (curl rc 28). Two attempts, same result.
- Likely cause: the egress proxy / YouTube tarpits unauthenticated
  InnerTube POSTs from this datacenter egress, or the request's TLS
  fingerprint is stalled. Not a code defect — the request bodies were
  built by `crates/yt-source/src/innertube.rs` and are unit-tested.
- Consequence: `clients.json` versions remain **placeholders**
  (WEB 2.20250101.00.00, ANDROID 19.09.37) until verified from a normal
  network (user's Windows machine or a residential egress). Do not treat
  them as working. The nightly canary (spec §Phase 3) must run where the
  POST actually completes.
