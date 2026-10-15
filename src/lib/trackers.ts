import type { Tracker, TrackerKind } from "./types";

export const TRACKER_TABS: { kind: TrackerKind; label: string; empty: string }[] = [
  { kind: "package", label: "Packages", empty: "No packages. Paste a tracking number below, or tell BYTE “track 1Z…”." },
  { kind: "bill", label: "Bills", empty: "No bills or subscriptions. Add one below, or tell BYTE “add Netflix $15.49 a month on the 12th”." },
  { kind: "event", label: "Dates", empty: "No birthdays or dates. Add one below, or tell BYTE “Sam's birthday is March 3”." },
  { kind: "upkeep", label: "Maintenance", empty: "No car or home maintenance. Add one below, or tell BYTE “change the furnace filter every 3 months”." },
];

const DAY = 86_400_000;

/** "YYYY-MM-DD" → local midnight (ms). */
export function dayMs(date: string): number {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).getTime();
}

/** "today", "tomorrow", "in 5 days", "3 days ago", "Oct 12" (like Rust's `in_days`). */
export function inDays(date: string | null, now: number): string {
  if (!date) return "";
  const today = new Date(now);
  const d = Math.round((dayMs(date) - new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime()) / DAY);
  if (d === 0) return "today";
  if (d === 1) return "tomorrow";
  if (d === -1) return "yesterday";
  if (d < 0) return `${-d} days ago`;
  if (d < 14) return `in ${d} days`;
  return new Date(dayMs(date)).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function isOverdue(t: Tracker, now: number): boolean {
  const today = new Date(now);
  return !!t.next && dayMs(t.next) < new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
}

/** "$15.49", "€9.99" (like Rust's `money_text`). */
export function moneyText(v: number, currency: string): string {
  const whole = Math.abs(v - Math.round(v)) < 0.005;
  const s = v.toLocaleString("en-US", { minimumFractionDigits: whole ? 0 : 2, maximumFractionDigits: whole ? 0 : 2 });
  if (currency === "USD" || !currency) return `$${s}`;
  if (currency === "EUR") return `€${s}`;
  if (currency === "GBP") return `£${s}`;
  return `${s} ${currency}`;
}

export function perMonth(t: Tracker): number {
  const a = t.amount ?? 0;
  if (t.cycle === "weekly") return (a * 52) / 12;
  if (t.cycle === "quarterly") return a / 3;
  if (t.cycle === "yearly") return a / 12;
  return a;
}

/** Monthly and yearly totals of open bills (null when they're in different currencies). */
export function billTotals(all: Tracker[]): { month: number; year: number; currency: string } | null {
  const bills = all.filter((t) => t.kind === "bill" && !t.done);
  if (bills.length === 0) return null;
  const currency = bills[0].currency;
  if (bills.some((b) => b.currency !== currency)) return null;
  const month = bills.reduce((s, b) => s + perMonth(b), 0);
  return { month, year: month * 12, currency };
}

export function everyText(t: Pick<Tracker, "everyMonths" | "everyDays">): string {
  const m = t.everyMonths;
  const d = t.everyDays;
  if (m === 1) return "every month";
  if (m === 12) return "every year";
  if (m && m % 12 === 0) return `every ${m / 12} years`;
  if (m) return `every ${m} months`;
  if (d === 7) return "every week";
  if (d && d % 7 === 0) return `every ${d / 7} weeks`;
  if (d) return `every ${d} days`;
  return "";
}

export const blankTracker = (kind: TrackerKind): Tracker => ({
  id: 0,
  kind,
  name: "",
  next: null,
  noticeDays: null,
  notes: "",
  done: false,
  carrier: "",
  number: "",
  amount: null,
  currency: "USD",
  cycle: kind === "bill" ? "monthly" : "",
  person: "",
  occasion: kind === "event" ? "birthday" : "",
  ideas: [],
  budget: null,
  everyDays: null,
  everyMonths: kind === "upkeep" ? 3 : null,
  lastDone: null,
  link: "",
  notifiedFor: null,
});

/** "$1,299.99" / "15" → a positive number, or null. */
export function parseAmount(text: string): number | null {
  const n = Number(text.replace(/[^0-9.]/g, ""));
  return Number.isFinite(n) && n > 0 ? n : null;
}
