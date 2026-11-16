# SPIKE.md — Phase 0 (specified, NOT run)

Goal: prove Tauri window with transparent webview; mpv playing a local file
into a child window under the webview on Windows; rectangle sync from UI.

## Protocol (Windows only)

1. Build release, launch, cold-start timer to interactive home.
2. Play a local 1080p file via mpv child window; UI reports its rect
   (physical px = display scale × page zoom) over `report_video_rect`.
3. Resize / DPI change / fullscreen / move to second monitor; check the
   child window follows, z-order stays beneath the webview, no flicker.
4. Sample Private Bytes of the whole process tree every 5 s
   (`scripts/measure.ps1`).

## Result

Not run in this session — no Windows machine, no WebView2, no libmpv on
this Linux VM. Do not claim Phase 0 done until this file carries real
numbers, a date, and a commit hash.
