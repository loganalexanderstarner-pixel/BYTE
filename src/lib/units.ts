// Kitchen units on the recipe card: US measures (tsp, tbsp, cup, oz, lb, °F)
// or metric (mL, g, °C). Mirrors src-tauri/src/units.rs, which converts
// recipes when they're made; this lets the card switch either way.
import type { Recipe } from "./types";

type Unit = "ml" | "l" | "g" | "kg" | "tsp" | "tbsp" | "cup" | "floz" | "oz" | "lb";

const UNITS: Record<string, Unit> = {
  ml: "ml", milliliter: "ml", milliliters: "ml", millilitre: "ml", millilitres: "ml",
  l: "l", liter: "l", liters: "l", litre: "l", litres: "l",
  g: "g", gr: "g", gram: "g", grams: "g",
  kg: "kg", kilogram: "kg", kilograms: "kg",
  tsp: "tsp", teaspoon: "tsp", teaspoons: "tsp", t: "tsp",
  tbsp: "tbsp", tablespoon: "tbsp", tablespoons: "tbsp", tbs: "tbsp",
  cup: "cup", cups: "cup", c: "cup",
  "fl oz": "floz", "fluid ounce": "floz", "fluid ounces": "floz",
  oz: "oz", ounce: "oz", ounces: "oz",
  lb: "lb", lbs: "lb", pound: "lb", pounds: "lb",
};

const unitOf = (s: string): Unit | undefined => UNITS[s.trim().replace(/\.$/, "").toLowerCase()];

/** Grams per US cup for ingredients measured by volume in US recipes. */
const DENSITY: [string, number][] = [
  ["powdered sugar", 120], ["icing sugar", 120], ["confectioners", 120], ["brown sugar", 213], ["sugar", 200],
  ["bread flour", 127], ["whole wheat flour", 120], ["almond flour", 96], ["flour", 120], ["cocoa", 85], ["oats", 90],
  ["rice", 185], ["honey", 340], ["maple syrup", 320], ["syrup", 340], ["chocolate chips", 170], ["cornstarch", 128],
  ["cornmeal", 138], ["breadcrumbs", 108], ["parmesan", 100], ["shredded cheese", 113], ["nuts", 120], ["walnuts", 120],
  ["pecans", 110], ["raisins", 150], ["peanut butter", 258], ["yogurt", 245], ["sour cream", 230],
];

const density = (item: string) => DENSITY.find(([k]) => item.toLowerCase().includes(k))?.[1];
const isButter = (item: string) => {
  const i = item.toLowerCase();
  return i.includes("butter") && !i.includes("peanut") && !i.includes("almond butter") && !i.includes("buttermilk");
};
const eighth = (x: number) => Math.max(0.125, Math.round(x * 8) / 8);
const niceMetric = (x: number) => (x >= 100 ? Math.round(x / 5) * 5 : x >= 10 ? Math.round(x) : Math.round(x * 10) / 10);

type Ing = Recipe["ingredients"][number];

function withNote(i: Ing, text: string): string {
  if (!i.note) return text;
  return i.note.includes(text) ? i.note : `${text}; ${i.note}`;
}

/** One ingredient in US measures (the metric amount goes in the note). */
export function ingredientToUS(i: Ing): Ing {
  const u = unitOf(i.unit);
  if (i.qty == null || !u) return i;
  const original = `${Math.round(i.qty * 100) / 100} ${i.unit.trim()}`;
  if (u === "ml" || u === "l") {
    const ml = u === "l" ? i.qty * 1000 : i.qty;
    const [qty, unit] = ml < 14 ? [eighth(ml / 4.93), "tsp"] : ml < 59 ? [eighth(ml / 14.79), "tbsp"] : [eighth(ml / 236.6), "cup"];
    return { ...i, qty, unit, note: withNote(i, original) };
  }
  if (u === "g" || u === "kg") {
    const g = u === "kg" ? i.qty * 1000 : i.qty;
    const d = density(i.item);
    let qty: number;
    let unit: string;
    if (isButter(i.item)) {
      const tbsp = g / 14.2;
      [qty, unit] = tbsp < 16 ? [Math.max(0.5, Math.round(tbsp * 2) / 2), "tbsp"] : [eighth(g / 227), "cup"];
    } else if (d) {
      const cups = g / d;
      [qty, unit] = cups < 0.25 ? [eighth(cups * 16), "tbsp"] : [eighth(cups), "cup"];
    } else if (g < 454) {
      [qty, unit] = [Math.max(1, Math.round((g / 28.35) * 2)) / 2, "oz"];
    } else {
      [qty, unit] = [Math.round((g / 453.6) * 4) / 4, "lb"];
    }
    return { ...i, qty, unit, note: withNote(i, original) };
  }
  return i;
}

/** One ingredient in metric (mL for liquids; g for weights, butter and dry baking goods). */
export function ingredientToMetric(i: Ing): Ing {
  const u = unitOf(i.unit);
  if (i.qty == null || !u) return i;
  // The exact metric amount is often already in the note ("80 ml"): use it.
  const m = /^\s*(\d+(?:\.\d+)?)\s*(g|kg|ml|l)\b[;,]?\s*/i.exec(i.note ?? "");
  if (m && u !== "g" && u !== "kg" && u !== "ml" && u !== "l") {
    return { ...i, qty: Number(m[1]), unit: m[2].toLowerCase(), note: i.note.slice(m[0].length) };
  }
  const ml = u === "tsp" ? i.qty * 5 : u === "tbsp" ? i.qty * 15 : u === "cup" ? i.qty * 240 : u === "floz" ? i.qty * 30 : null;
  if (ml != null) {
    const d = density(i.item);
    const grams = isButter(i.item) ? (ml / 15) * 14.2 : d ? (ml / 240) * d : null;
    return grams != null && ml >= 15 ? { ...i, qty: niceMetric(grams), unit: "g" } : { ...i, qty: niceMetric(ml), unit: "ml" };
  }
  if (u === "oz") return { ...i, qty: niceMetric(i.qty * 28.35), unit: "g" };
  if (u === "lb") return { ...i, qty: niceMetric(i.qty * 453.6), unit: "g" };
  return i;
}

/** "Bake at 180°C" → "Bake at 350°F" (or back); text that gives both is left alone. */
export function temps(text: string, metric: boolean): string {
  const [from, to] = metric ? ["F", "C"] : ["C", "F"];
  if (!text || text.includes(`°${to}`)) return text;
  return text.replace(new RegExp(`(\\d+)\\s*°${from}`, "g"), (_, v: string) => {
    const n = Number(v);
    const c = metric ? ((n - 32) * 5) / 9 : (n * 9) / 5 + 32;
    const step = metric ? (c >= 100 ? 10 : 5) : c >= 200 ? 25 : 5;
    return `${Math.round(c / step) * step}°${to}`;
  });
}

/** Is the recipe written mostly in metric? */
export function isMetric(r: Recipe): boolean {
  let metric = 0;
  let us = 0;
  for (const i of r.ingredients) {
    const u = unitOf(i.unit);
    if (u === "ml" || u === "l" || u === "g" || u === "kg") metric++;
    else if (u) us++;
  }
  return metric > us;
}

/** The recipe in the chosen measures. Converting from the original each time avoids drift. */
export function convertRecipe(r: Recipe, metric: boolean): Recipe {
  return {
    ...r,
    ingredients: r.ingredients.map(metric ? ingredientToMetric : ingredientToUS),
    steps: r.steps.map((s) => ({ ...s, text: temps(s.text, metric), cue: temps(s.cue, metric) })),
    tips: r.tips.map((t) => temps(t, metric)),
  };
}
