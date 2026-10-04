import { describe, expect, it } from "vitest";

import { convertRecipe, ingredientToMetric, ingredientToUS, isMetric, temps } from "./units";
import type { Recipe } from "./types";

const ing = (qty: number, unit: string, item: string) => ({ qty, unit, item, note: "", have: false });

describe("kitchen units", () => {
  it("turns metric amounts into cups and spoons", () => {
    expect(ingredientToUS(ing(250, "ml", "milk"))).toMatchObject({ qty: 1, unit: "cup", note: "250 ml" });
    expect(ingredientToUS(ing(15, "mL", "olive oil"))).toMatchObject({ qty: 1, unit: "tbsp" });
    expect(ingredientToUS(ing(5, "ml", "vanilla"))).toMatchObject({ qty: 1, unit: "tsp" });
    expect(ingredientToUS(ing(240, "g", "all-purpose flour"))).toMatchObject({ qty: 2, unit: "cup" });
    expect(ingredientToUS(ing(115, "g", "butter"))).toMatchObject({ qty: 8, unit: "tbsp" });
    expect(ingredientToUS(ing(1, "kg", "potatoes"))).toMatchObject({ qty: 2.25, unit: "lb" });
    expect(ingredientToUS(ing(2, "cups", "water"))).toMatchObject({ qty: 2, unit: "cups", note: "" });
  });

  it("turns US amounts into metric", () => {
    expect(ingredientToMetric(ing(1, "cup", "milk"))).toMatchObject({ qty: 240, unit: "ml" });
    expect(ingredientToMetric(ing(2, "cups", "flour"))).toMatchObject({ qty: 240, unit: "g" });
    expect(ingredientToMetric(ing(4, "tbsp", "butter"))).toMatchObject({ qty: 57, unit: "g" });
    // The metric amount already in the note wins, and leaves the note.
    expect(ingredientToMetric({ ...ing(0.33, "cup", "whole milk"), note: "80 ml" })).toMatchObject({ qty: 80, unit: "ml", note: "" });
    expect(ingredientToMetric({ ...ing(5, "oz", "spinach"), note: "140 g; washed" })).toMatchObject({ qty: 140, unit: "g", note: "washed" });
  });

  it("converts temperatures in text", () => {
    expect(temps("Bake at 180°C for 25 minutes.", false)).toBe("Bake at 350°F for 25 minutes.");
    expect(temps("Bake at 350°F", true)).toBe("Bake at 180°C");
    expect(temps("Heat to 200°C (400°F).", false)).toBe("Heat to 200°C (400°F).");
  });

  it("switches a whole recipe both ways from the original", () => {
    const r = {
      title: "Banana bread", description: "", category: "baking", cuisine: "", servings: 8, prepMin: 15, cookMin: 60,
      difficulty: "Easy", equipment: [], ingredients: [ing(250, "g", "flour"), ing(115, "g", "butter"), ing(120, "ml", "milk")],
      steps: [{ text: "Bake at 180°C.", minutes: 60, cue: "" }], tips: [], substitutions: [], storage: "", image: "", sourceUrl: "", sourceName: "", emoji: "🍌",
    } as unknown as Recipe;
    expect(isMetric(r)).toBe(true);
    const us = convertRecipe(r, false);
    expect(us.ingredients.map((i) => i.unit)).toEqual(["cup", "tbsp", "cup"]);
    expect(us.steps[0].text).toBe("Bake at 350°F.");
    // Back to metric from the original, not from the rounded US amounts.
    expect(convertRecipe(r, true).ingredients[0]).toMatchObject({ qty: 250, unit: "g" });
  });
});
