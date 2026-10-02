/** One line for the list: whitespace squeezed, cut at 280 characters. */
export function clipPreview(text: string): string {
  const t = text.replace(/\s+/g, " ").trim();
  return t.length > 280 ? `${t.slice(0, 279)}…` : t;
}

/** "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago". */
export function clipAge(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  return d === 1 ? "yesterday" : `${d} days ago`;
}
