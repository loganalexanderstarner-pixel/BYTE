/** Helpers for the reader view: which file a source points at, and where the
 * cited passage is in that file's text. */

/** `file:///Users/me/Lease.pdf#page=3` → path and page (null for web links). */
export function fileSource(url: string): { path: string; page: number | null } | null {
  if (!url.startsWith("file://")) return null;
  try {
    const u = new URL(url);
    const page = /^#page=(\d+)$/.exec(u.hash)?.[1];
    return { path: decodeURIComponent(u.pathname), page: page ? Number(page) : null };
  } catch {
    return null;
  }
}

/**
 * Where `snippet` appears in `text` (whitespace and case don't have to match),
 * as [start, end) in `text`. Falls back to the `[Page N]` marker, then null.
 */
export function locate(text: string, snippet: string, page: number | null = null): [number, number] | null {
  const words = snippet.trim().split(/\s+/).filter(Boolean);
  // Try the whole snippet, then shorter starts (snippets can be cut mid-word).
  for (let n = Math.min(words.length, 40); n >= 4; n = Math.floor(n / 2)) {
    const parts = words.slice(0, n).map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
    const m = new RegExp(parts.join("\\s+"), "i").exec(text);
    if (m) return [m.index, m.index + m[0].length];
  }
  if (page !== null) {
    const m = new RegExp(`\\[(Page|Slide|Sheet) ${page}\\]`).exec(text);
    if (m) return [m.index, m.index + m[0].length];
  }
  return null;
}
