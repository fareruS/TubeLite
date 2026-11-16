# MEMORY.md — cache caps and memory budget

Every cache has a size cap and eviction policy. No unbounded cache ships.

| Cache | Cap | Eviction | Where |
|---|---|---|---|
| UI page cache (`ui/src/lib/pagecache.ts`) | 80 entries, TTL 5 min | Oldest (Map insertion order) | in-memory |
| In-memory resolved-URL cache | 8 entries, 4 h TTL | Oldest / on expiry or 403 | Rust, Phase 3 |
| mpv demuxer (audio) | 64 MiB fwd / 64 MiB back | mpv demuxer | `MpvConfig::audio_only` |
| mpv demuxer (video) | 128 MiB fwd / 64 MiB back (starting point) | mpv demuxer | `MpvConfig::video` |
| mpv disk cache | bounded; max cache-dir size enforced at startup | oldest files | app cache dir, Phase 2 |
| Thumbnails | disk cache with size cap (Phase 4) | oldest | app cache dir |
| Captions cache (SQLite) | per-video+lang, TTL in Phase 3 | stale rows pruned | `captions_cache` |
| Metadata cache (SQLite) | TTL per row | stale rows pruned | `metadata_cache` |
| Caption files (VTT) | one file per video+lang, in the app cache dir | oldest files with the mpv disk cache | `~/.cache/tubelite/captions/` |

## Budget (targets, Windows 11, whole process tree)

Idle <150 MB · 1080p playing <300 MB · hidden-to-tray <90 MB ·
cold start <1.5 s · CPU <3% of one core above decoder · <5% frames over
20 ms scrolling a 500-row list at 4x CPU throttle.

## Measurements

None yet on Windows. This session (Linux VM, 2026-11-08): no process-tree
memory measurement was run, because there is no built app with video
playback here yet. Record the first real numbers in
`docs/measurements/<date>-<commit>.md` using `scripts/measure.sh` /
`scripts/measure.ps1` — do not copy target numbers into a report as if measured.
