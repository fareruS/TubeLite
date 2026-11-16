// Bounded in-memory page cache: MAX_ENTRIES = 80, TTL = 5 minutes, evict oldest.
// Revalidate in the background on revisit. Clear all on sign-in/out.
// Measured rule: an unbounded cache is how a feed grows RAM without bound.
export const MAX_ENTRIES = 80;
export const TTL_MS = 5 * 60 * 1000;
type Entry = { value: unknown; at: number };
const cache = new Map<string, Entry>();
export function pageCacheGet<T>(key: string): T | undefined {
  const e = cache.get(key);
  if (!e) return undefined;
  if (Date.now() - e.at > TTL_MS) { cache.delete(key); return undefined; }
  return e.value as T;
}
export function pageCacheSet(key: string, value: unknown): void {
  if (cache.size >= MAX_ENTRIES) { const oldest = cache.keys().next().value; if (oldest) cache.delete(oldest); }
  cache.set(key, { value, at: Date.now() });
}
export function pageCacheClear(): void { cache.clear(); }
