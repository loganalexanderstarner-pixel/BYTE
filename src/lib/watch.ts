/** Helpers for feeds and page watchers in the ✅ panel (Rust `feeds.rs`, `watchers.rs`). */
import type { Watcher } from "./types";

/** "$179", "$189.50", "€50", "12.50 CAD" (the same as Rust `watchers::money`). */
export function money(v: number, currency: string): string {
  const n = Math.abs(v % 1) < 0.005 ? v.toFixed(0) : v.toFixed(2);
  switch (currency) {
    case "":
    case "USD":
      return `$${n}`;
    case "EUR":
      return `€${n}`;
    case "GBP":
      return `£${n}`;
    case "JPY":
      return `¥${n}`;
    default:
      return `${n} ${currency}`;
  }
}

function every(h: number): string {
  return h === 1 ? "every hour" : h === 24 ? "once a day" : `every ${h} hours`;
}

/** "Price at or below $150 · every 6 hours", "Changes · once a day". */
export function watchSummary(w: Watcher): string {
  const what = w.kind === "change" ? "Changes" : w.target != null ? `Price at or below ${money(w.target, w.currency)}` : "Price drops";
  return `${what} · ${every(w.everyHours)}`;
}

/** "just now", "5m ago", "3h ago", "2d ago". */
export function ago(t: number | null | undefined, now: number): string {
  if (t == null) return "";
  const m = Math.floor((now - t) / 60_000);
  if (m < 1) return "just now";
  if (m < 60) return `${m}m ago`;
  if (m < 24 * 60) return `${Math.floor(m / 60)}h ago`;
  return `${Math.floor(m / (24 * 60))}d ago`;
}

/** Hours to check a page, as offered in the panel. */
export const CHECK_EVERY: { hours: number; label: string }[] = [
  { hours: 1, label: "Every hour" },
  { hours: 6, label: "Every 6 hours" },
  { hours: 24, label: "Once a day" },
];

/** A price the user typed ("$1,299.99", "150") or null. */
export function parseTarget(s: string): number | null {
  const n = Number(s.replace(/[$€£¥,\s]/g, ""));
  return s.trim() && Number.isFinite(n) && n > 0 ? n : null;
}
