import { describe, expect, it } from "vitest";

import { formatQty, groceryText, ingredientLine, minutesText, recipeDocSpec, recipeEmoji, recipeText } from "./recipe";
import type { Recipe } from "./types";

const r: Recipe = {
  title: "Spinach & Feta Frittata",
  description: "Fluffy and golden.",
  category: "breakfast",
  cuisine: "Greek",
  servings: 4,
  prepMin: 10,
  cookMin: 15,
  difficulty: "Easy",
  equipment: ["10-inch oven-safe skillet"],
  ingredients: [
    { qty: 8, unit: "", item: "large eggs", note: "", have: true },
    { qty: 0.5, unit: "cup", item: "crumbled feta", note: "60 g", have: true },
    { qty: 250, unit: "g", item: "baby spinach", note: "", have: false },
    { qty: null, unit: "", item: "salt", note: "to taste", have: false },
  ],
  steps: [
    { text: "Heat the oven to 400°F (200°C).", minutes: null, cue: "" },
    { text: "Bake.", minutes: 12, cue: "the center barely jiggles" },
  ],
  tips: ["Pull it early; it keeps cooking."],
  substitutions: ["Goat cheese for feta"],
  storage: "Fridge 3 days",
  image: "",
  sourceUrl: "https://example.com/frittata",
  sourceName: "example.com",
  emoji: "",
};

describe("recipes", () => {
  it("formats quantities as cooks write them", () => {
    expect(formatQty(1.5)).toBe("1½");
    expect(formatQty(0.33)).toBe("⅓");
    expect(formatQty(2)).toBe("2");
    expect(formatQty(0.98)).toBe("1");
    expect(formatQty(2.66)).toBe("2⅔");
    expect(formatQty(312.5, "g")).toBe("315");
    expect(formatQty(12.4, "ml")).toBe("12");
    expect(formatQty(0.75, "kg")).toBe("0.8");
  });

  it("scales ingredients to the servings chosen", () => {
    expect(ingredientLine(r.ingredients[0], 4, 2)).toBe("4 large eggs");
    expect(ingredientLine(r.ingredients[1], 4, 6)).toBe("¾ cup crumbled feta, 60 g");
    expect(ingredientLine(r.ingredients[2], 4, 6)).toBe("375 g baby spinach");
    expect(ingredientLine(r.ingredients[3], 4, 8)).toBe("salt, to taste");
  });

  it("reads times and picks an emoji", () => {
    expect(minutesText(25)).toBe("25 min");
    expect(minutesText(75)).toBe("1 h 15 min");
    expect(minutesText(120)).toBe("2 h");
    expect(recipeEmoji(r)).toBe("🍳");
    expect(recipeEmoji({ emoji: "", category: "coffee" })).toBe("☕");
  });

  it("becomes text and a document", () => {
    const text = recipeText(r, 2);
    expect(text).toContain("Serves 2 · 25 min");
    expect(text).toContain("- 4 large eggs");
    expect(text).toContain("2. Bake. (the center barely jiggles)");
    const spec = recipeDocSpec(r);
    expect(spec.sections.map((s) => s.title)).toEqual(["You'll need", "Ingredients", "Method", "Chef's tips", "Swaps and storage"]);
    expect(spec.sources).toEqual([{ n: 1, title: "example.com", url: "https://example.com/frittata" }]);
    expect(groceryText({ days: [], have: [], grocery: [{ aisle: "Produce", items: ["2 limes"] }, { aisle: "Dairy", items: ["milk"] }] })).toBe("Produce\n- 2 limes\n\nDairy\n- milk");
  });
});
