import { describe, expect, it } from "vitest";

import { BALANCED, PRESETS, presetOf, sliderWord } from "./personality";

describe("personality", () => {
  it("knows its presets", () => {
    expect(presetOf(undefined)).toBe("balanced");
    expect(presetOf(PRESETS.find((p) => p.id === "coach")!.p)).toBe("coach");
    expect(presetOf({ ...BALANCED, humor: 5 })).toBeNull();
    expect(new Set(PRESETS.map((p) => JSON.stringify(p.p))).size).toBe(PRESETS.length);
  });
  it("describes slider positions", () => {
    expect(sliderWord(1, "Brief", "Thorough")).toBe("Very brief");
    expect(sliderWord(3, "Brief", "Thorough")).toBe("Normal");
    expect(sliderWord(4, "Casual", "Formal")).toBe("A bit formal");
  });
});
