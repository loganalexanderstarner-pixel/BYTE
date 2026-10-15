import { describe, expect, it } from "vitest";

import { budgetLabel, fitLabel, fitTone, isEmptyOverride, meterPercent, paramsLabel, withOverride, withoutOverride } from "./tuning";

describe("tuning helpers", () => {
  it("maps fit to a theme tone and label", () => {
    expect(fitTone("great")).toBe("ok");
    expect(fitTone("tight")).toBe("warn");
    expect(fitTone("no")).toBe("danger");
    expect(fitLabel("no")).toBe("Too big for this Mac");
  });

  it("labels parameter counts", () => {
    expect(paramsLabel(8.03)).toBe("8.0B");
    expect(paramsLabel(0.6)).toBe("0.6B");
    expect(paramsLabel(235.2)).toBe("235B");
    expect(paramsLabel(null)).toBe("Unknown size");
    expect(paramsLabel(0)).toBe("Unknown size");
  });

  it("labels thinking budgets", () => {
    expect(budgetLabel(null)).toBe("Recommended");
    expect(budgetLabel(undefined)).toBe("Recommended");
    expect(budgetLabel(-1)).toBe("No limit");
    expect(budgetLabel(1024)).toBe("1,024 tokens");
  });

  it("computes clamped meter percentages", () => {
    expect(meterPercent(4, 16)).toBe(25);
    expect(meterPercent(20, 16)).toBe(100);
    expect(meterPercent(1, 0)).toBe(0);
    expect(meterPercent(null, 16)).toBe(0);
    expect(meterPercent(-1, 16)).toBe(0);
  });

  it("merges an override without mutating the input", () => {
    const before = { a: { temperature: 0.7 } };
    const after = withOverride(before, "a", { topP: 0.9 });
    expect(after).toEqual({ a: { temperature: 0.7, topP: 0.9 } });
    expect(before).toEqual({ a: { temperature: 0.7 } });
    expect(withOverride(undefined, "b", { thinkingBudget: -1 })).toEqual({ b: { thinkingBudget: -1 } });
  });

  it("drops fields reset to recommended and removes empty overrides", () => {
    const o = { a: { temperature: 0.7, topP: 0.9 }, b: { systemExtra: "Be brief." } };
    expect(withOverride(o, "a", { temperature: null })).toEqual({ a: { topP: 0.9 }, b: { systemExtra: "Be brief." } });
    expect(withOverride(o, "a", { temperature: null, topP: null })).toEqual({ b: { systemExtra: "Be brief." } });
    expect(withOverride(o, "b", { systemExtra: "   " })).toEqual({ a: { temperature: 0.7, topP: 0.9 } });
    expect(isEmptyOverride({ temperature: null, systemExtra: "" })).toBe(true);
    expect(isEmptyOverride({ temperature: 0 })).toBe(false);
  });

  it("removes a key without mutating the input", () => {
    const o = { a: { temperature: 1 }, b: { topP: 0.5 } };
    expect(withoutOverride(o, "a")).toEqual({ b: { topP: 0.5 } });
    expect(o.a).toEqual({ temperature: 1 });
    expect(withoutOverride(null, "a")).toEqual({});
  });
});
