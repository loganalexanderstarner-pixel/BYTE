import { describe, expect, it } from "vitest";

import { suggest } from "./settingsSuggest";

const titles = ["Auto-pause", "Always on top", "Appearance", "Wake word detector", "Hey BYTE microphone", "Test log", "Speaker for answers", "After BYTE answers Hey BYTE"];

describe("settings suggestions", () => {
  it("lists the settings that start with the typed letters first", () => {
    expect(suggest(titles, "a")).toEqual(["Auto-pause", "Always on top", "Appearance", "After BYTE answers Hey BYTE", "Speaker for answers"]);
  });
  it("finds a setting by a later word", () => {
    expect(suggest(titles, "mic")).toEqual(["Hey BYTE microphone"]);
  });
  it("is empty for nothing typed and ignores the exact name already typed", () => {
    expect(suggest(titles, "  ")).toEqual([]);
    expect(suggest(titles, "test log")).toEqual([]);
  });
  it("stops at the maximum", () => {
    expect(suggest(titles, "a", 2)).toHaveLength(2);
  });
});
