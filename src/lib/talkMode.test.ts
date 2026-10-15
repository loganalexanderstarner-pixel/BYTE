import { describe, expect, it } from "vitest";

import { ADULT_WARNING, needsAdultWarning, TALK_MODES } from "./talkMode";

describe("talk modes", () => {
  it("lists every mode the backend knows, and only those", () => {
    expect(TALK_MODES.map((m) => m.id)).toEqual(["", "casual", "direct", "unhinged", "auto"]);
  });
  it("warns before Unhinged, once", () => {
    expect(needsAdultWarning("unhinged", false)).toBe(true);
    expect(needsAdultWarning("unhinged", undefined)).toBe(true);
    expect(needsAdultWarning("unhinged", true)).toBe(false);
    expect(needsAdultWarning("casual", false)).toBe(false);
    expect(needsAdultWarning("auto", false)).toBe(false);
  });
  it("the warning says it is 18+ and to be careful", () => {
    expect(ADULT_WARNING).toMatch(/18\+/);
    expect(ADULT_WARNING).toMatch(/careful/i);
    expect(ADULT_WARNING).toMatch(/entertainment/i);
    expect(ADULT_WARNING).toMatch(/don't take its advice/i);
  });
});
