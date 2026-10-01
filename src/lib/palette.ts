/** The command palette's search (⌘K): fuzzy, in order, with word starts and runs scoring higher. */

export interface PaletteItem {
  id: string;
  label: string;
  /** Shown on the right (a shortcut, or what kind of thing it is). */
  hint?: string;
  /** Extra words that should find it ("preferences" for Settings). */
  keywords?: string;
  group: "Actions" | "Chats" | "Settings" | "Modes" | "Themes";
}

/** How well `query` matches `text` (0 = not at all). Every query letter must appear, in order. */
export function fuzzyScore(query: string, text: string): number {
  const q = query.toLowerCase().replace(/\s+/g, " ").trim();
  if (!q) return 1;
  const t = text.toLowerCase();
  const at = t.indexOf(q);
  // A plain substring beats any scattered match; at a word start, more so.
  if (at >= 0) return 1000 - at + (at === 0 || /\W/.test(t[at - 1]) ? 200 : 0);
  let score = 0;
  let ti = 0;
  let run = 0;
  for (const ch of q) {
    if (ch === " ") continue;
    const found = t.indexOf(ch, ti);
    if (found < 0) return 0;
    run = found === ti ? run + 1 : 0;
    score += 10 + run * 5 + (found === 0 || /\W/.test(t[found - 1]) ? 15 : 0) - Math.min(found - ti, 10);
    ti = found + 1;
  }
  return Math.max(1, score);
}

/** The items that match, best first (ties keep their order); everything when the query is empty. */
export function rankItems(items: PaletteItem[], query: string, limit = 12): PaletteItem[] {
  if (!query.trim()) return items.slice(0, limit);
  return items
    .map((it, i) => ({ it, i, s: Math.max(fuzzyScore(query, it.label), it.keywords ? fuzzyScore(query, it.keywords) * 0.8 : 0) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s || a.i - b.i)
    .slice(0, limit)
    .map((x) => x.it);
}
