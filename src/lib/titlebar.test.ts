import { describe, expect, it } from "vitest";

import { lightness, parseRgb } from "./titlebar";

describe("title bar theme", () => {
  it("reads the colour a browser reports", () => {
    expect(parseRgb("rgb(15, 20, 32)")).toEqual([15, 20, 32]);
    expect(parseRgb("rgba(244, 246, 251, 1)")).toEqual([244, 246, 251]);
    expect(parseRgb("transparent")).toBeNull();
  });
  it("calls the dark themes dark and the light ones light", () => {
    expect(lightness([15, 20, 32])).toBeLessThan(0.5); // Midnight
    expect(lightness([0, 0, 0])).toBeLessThan(0.5); // High Contrast
    expect(lightness([244, 246, 251])).toBeGreaterThan(0.5); // Light
    expect(lightness([250, 246, 238])).toBeGreaterThan(0.5); // Paper
  });
});
