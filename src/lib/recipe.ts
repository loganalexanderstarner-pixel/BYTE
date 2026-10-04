// Recipe helpers: scaling to a different number of servings, friendly
// fractions ("1½ cups"), times, and the recipe as text (Copy) or a document.

import type { DocSpec } from "./docs/spec";
import type { MealPlan, Recipe } from "./types";

const FRACTIONS: [number, string][] = [
  [1 / 8, "⅛"],
  [1 / 4, "¼"],
  [1 / 3, "⅓"],
  [3 / 8, "⅜"],
  [1 / 2, "½"],
  [5 / 8, "⅝"],
  [2 / 3, "⅔"],
  [3 / 4, "¾"],
  [7 / 8, "⅞"],
];

/** Units measured by weight or volume in metric: shown as decimals, not fractions. */
const METRIC = /^(g|kg|ml|l|mg|cl|dl|gram|grams|litre|liter|litres|liters|millilitre|milliliter)s?$/i;

/** A quantity for display: "1½", "⅓", "2", "250", "0.5" (metric decimals). */
export function formatQty(q: number, unit = ""): string {
  if (!isFinite(q) || q <= 0) return "";
  if (METRIC.test(unit.trim())) {
    return q >= 100 ? String(Math.round(q / 5) * 5) : q >= 10 ? String(Math.round(q)) : String(Math.round(q * 10) / 10);
  }
  const whole = Math.floor(q);
  const frac = q - whole;
  if (frac < 0.06) return String(whole || (q > 0 ? "⅛" : "0"));
  if (frac > 0.94) return String(whole + 1);
  const [, sym] = FRACTIONS.reduce((best, f) => (Math.abs(f[0] - frac) < Math.abs(best[0] - frac) ? f : best));
  return whole ? `${whole}${sym}` : sym;
}

/** An ingredient line at `servings` (the recipe was written for `base`). */
export function ingredientLine(i: Recipe["ingredients"][number], base: number, servings: number): string {
  const q = i.qty != null ? formatQty((i.qty * servings) / base, i.unit) : "";
  return [q, i.unit, i.item].filter(Boolean).join(" ") + (i.note ? `, ${i.note}` : "");
}

export function minutesText(m: number | null | undefined): string {
  if (!m) return "";
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const r = m % 60;
  return r ? `${h} h ${r} min` : `${h} h`;
}

export function totalMinutes(r: Recipe): number | null {
  const t = (r.prepMin ?? 0) + (r.cookMin ?? 0);
  return t || null;
}

/** Emoji for a recipe without a photo. */
export function recipeEmoji(r: Pick<Recipe, "emoji" | "category">): string {
  if (r.emoji) return r.emoji;
  const c = r.category.toLowerCase();
  if (c.includes("coffee") || c.includes("drink")) return "☕";
  if (c.includes("bak") || c.includes("dessert")) return "🧁";
  if (c.includes("breakfast")) return "🍳";
  if (c.includes("salad")) return "🥗";
  if (c.includes("soup")) return "🍲";
  return "🍽️";
}

/** The recipe as plain text (Copy, notes apps). */
export function recipeText(r: Recipe, servings = r.servings): string {
  const lines = [`${r.title}`, r.description, "", `Serves ${servings}${totalMinutes(r) ? ` · ${minutesText(totalMinutes(r))}` : ""}`, "", "Ingredients"];
  for (const i of r.ingredients) lines.push(`- ${ingredientLine(i, r.servings, servings)}`);
  lines.push("", "Method");
  r.steps.forEach((s, k) => lines.push(`${k + 1}. ${s.text}${s.cue ? ` (${s.cue})` : ""}`));
  if (r.tips.length) lines.push("", "Chef's tips", ...r.tips.map((t) => `- ${t}`));
  if (r.sourceUrl) lines.push("", `Based on ${r.sourceName || r.sourceUrl}: ${r.sourceUrl}`);
  return lines.join("\n");
}

/** The recipe as a document for the PDF renderer. */
export function recipeDocSpec(r: Recipe, servings = r.servings): DocSpec {
  const meta = [`Serves ${servings}`, minutesText(r.prepMin) && `Prep ${minutesText(r.prepMin)}`, minutesText(r.cookMin) && `Cook ${minutesText(r.cookMin)}`, r.difficulty].filter(Boolean).join(" · ");
  const sections: DocSpec["sections"] = [
    { title: "Ingredients", blocks: [{ type: "bullets", items: r.ingredients.map((i) => ingredientLine(i, r.servings, servings)) }] },
    { title: "Method", blocks: [{ type: "numbered", items: r.steps.map((s) => `${s.text}${s.cue ? ` Look for: ${s.cue}.` : ""}`) }] },
  ];
  if (r.equipment.length) sections.unshift({ title: "You'll need", blocks: [{ type: "bullets", items: r.equipment }] });
  if (r.tips.length) sections.push({ title: "Chef's tips", blocks: [{ type: "bullets", items: r.tips }] });
  const extra = [...r.substitutions.map((s) => `Swap: ${s}`), ...(r.storage ? [`Storage: ${r.storage}`] : [])];
  if (extra.length) sections.push({ title: "Swaps and storage", blocks: [{ type: "bullets", items: extra }] });
  return {
    kind: "pdf",
    title: r.title,
    subtitle: [r.description, meta].filter(Boolean).join(" — "),
    sections,
    sources: r.sourceUrl ? [{ n: 1, title: r.sourceName || r.sourceUrl, url: r.sourceUrl }] : [],
  };
}

/** The grocery list as text (Copy), grouped by aisle. */
export function groceryText(plan: MealPlan): string {
  return plan.grocery.map((a) => [a.aisle, ...a.items.map((i) => `- ${i}`)].join("\n")).join("\n\n");
}
