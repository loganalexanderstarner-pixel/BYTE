/** 9_001_752_960 → "9.0 GB" (decimal units, like Finder). */
export function bytes(n: number, digits = 1): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log10(n) / 3));
  const v = n / 10 ** (i * 3);
  return `${v.toFixed(i === 0 ? 0 : digits)} ${units[i]}`;
}

/** Rounds RAM to the marketing size: 17_179_869_184 → "16 GB". */
export function ramSize(n: number): string {
  return `${Math.round(n / 2 ** 30)} GB`;
}

/** 95 → "1 min 35 s", 12 → "12 s". */
export function duration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  const s = Math.round(seconds);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ${s % 60} s`;
  const h = Math.floor(m / 60);
  return `${h} h ${m % 60} min`;
}

export function eta(done: number, total: number, perSec: number): string {
  if (perSec <= 0 || total <= done) return "—";
  return duration((total - done) / perSec);
}

export function tokensPerSec(n: number): string {
  return n >= 100 ? `${Math.round(n)} tok/s` : `${n.toFixed(1)} tok/s`;
}

export function contextLabel(tokens: number): string {
  return `${Math.round(tokens / 1024)}k`;
}

/** First line of a message, trimmed to a sidebar-friendly title. */
export function titleFrom(text: string, max = 48): string {
  const line = text.replace(/\s+/g, " ").trim();
  if (line.length <= max) return line || "New chat";
  const cut = line.slice(0, max);
  const space = cut.lastIndexOf(" ");
  return `${(space > max * 0.6 ? cut.slice(0, space) : cut).trimEnd()}…`;
}
