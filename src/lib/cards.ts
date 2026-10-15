// Pure helpers for the reviews, prices and game-hint cards.
import type { Offer } from "./types";

/** "$229.99", "€1,299.00"; a plain number when the currency is unknown. */
export function priceText(price: number, currency: string): string {
  if (/^[A-Z]{3}$/.test(currency)) {
    try {
      return new Intl.NumberFormat("en-US", { style: "currency", currency }).format(price);
    } catch {
      /* unknown code: fall through */
    }
  }
  return `${price.toFixed(2)}${currency ? ` ${currency}` : ""}`;
}

/** A rating on any scale as stars out of 5, rounded to a half. */
export function outOfFive(value: number, best: number): number {
  if (best <= 0) return 0;
  return Math.round((value / best) * 5 * 2) / 2;
}

/** "Checked just now", "Checked 12 min ago", "Checked 3 h ago", "Checked Mar 4". */
export function checkedText(iso: string, now = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const min = Math.max(0, Math.round((now - t) / 60000));
  if (min < 2) return "Checked just now";
  if (min < 60) return `Checked ${min} min ago`;
  if (min < 24 * 60) return `Checked ${Math.round(min / 60)} h ago`;
  return `Checked ${new Date(t).toLocaleDateString("en-US", { month: "short", day: "numeric" })}`;
}

/** How much more than the cheapest an offer costs ("+$20.00"), or "" for the cheapest. */
export function overCheapest(offer: Offer, cheapest: Offer): string {
  const d = offer.price - cheapest.price;
  return d > 0.005 ? `+${priceText(d, offer.currency)}` : "";
}

/** "In stock", "Out of stock", or "" when the page didn't say. */
export function stockText(inStock: boolean | null): string {
  return inStock === null ? "" : inStock ? "In stock" : "Out of stock";
}

/** Hints shown so far → how many after tapping "Next hint" (never past the last). */
export function nextHint(shown: number, total: number): number {
  return Math.min(total, shown + 1);
}
