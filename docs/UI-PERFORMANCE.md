# UI-PERFORMANCE.md

Every rule below came from measured lag elsewhere (Limusic's
`docs/UI-PERFORMANCE.md`, cited in `limusic-analysis/ANALISIS_LIMUSIC.md`).
Each rule must carry its own before/after numbers in the same commit that
introduces the effect. **No numbers have been measured for this app yet** —
the numbers quoted are Limusic's, as a reference to beat, not this app's.

1. **Window long lists.** Render only viewport slice + overscan 10
   (`ui/src/lib/rows.ts`). Reference: 1,000 rows = 23,132 DOM nodes = 488 MiB;
   5,000 rows = 115,132 nodes = 1,160 MiB without windowing.
2. **Bounded page cache.** 80 entries, 5 min TTL, evict oldest
   (`ui/src/lib/pagecache.ts`).
3. **Per-tick writes land on a leaf element.** Never write a custom property
   to `<html>` per tick / per pointer move (one write = 160–200 ms + ~10 MB
   never returned on WebKitGTK, per Limusic). Commit root tokens once on release.
4. **Hover animates colour only.** No transform/scale on repeated cards/rows.
   Rows carry no transition; reveal controls with `invisible group-hover:visible`.
   Reference: Home 46–52% frames >20 ms → 7–9% after removing card zoom.
5. **Blur a small source, once.** Backdrop from a 96 px thumbnail, one clipped
   wrapper. No `backdrop-filter` over the whole window or over repainting
   content; dialogs use plain `bg-black/80`.
6. **rAF only while output is visible and changing.** Gate it; idle otherwise.
   Pause rAF when the Watch page is hidden / window minimised.
7. **Layout is read on demand**, never at mount (scrollWidth/getBoundingClientRect
   at mount forces a synchronous full layout).
8. **Wheel/touch listeners passive** unless preventDefault is truly needed,
   then bind only while needed.
9. **Thumbnails at displayed size**, `loading=lazy`, `decoding=async`.
   Never maxres in a list.
10. **Position ticks at ~4 Hz**, discontinuities (seek, item change) immediately.
11. **Measure on a weak machine** (CPU throttled 4x+), with video playing.
