// Windowing: render only the slice around the viewport + OVERSCAN = 10 rows,
// stand in for the rest with padTop/padBottom so the scrollbar stays honest.
// Row height is fixed and measured, never assumed — ROW_PX is set from a real
// measurement in docs/UI-PERFORMANCE.md when the list ships (Phase 4).
export const OVERSCAN = 10;
export function windowedSlice<T>(items: readonly T[], scrollTop: number, viewportHeight: number, rowPx: number): { start: number; end: number; padTop: number; padBottom: number } {
  const start = Math.max(0, Math.floor(scrollTop / rowPx) - OVERSCAN);
  const end = Math.min(items.length, Math.ceil((scrollTop + viewportHeight) / rowPx) + OVERSCAN);
  return { start, end, padTop: start * rowPx, padBottom: (items.length - end) * rowPx };
}
