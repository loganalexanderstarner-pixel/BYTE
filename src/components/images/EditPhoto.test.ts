import { describe, expect, it } from "vitest";

import { cropRect, presetPrompt } from "./EditPhoto";

describe("cropRect", () => {
  it("takes the biggest rectangle of the shape and centres it", () => {
    const r = cropRect(2000, 1000, 1, 1, 0.5, 0.5);
    expect(r.w * 2000).toBeCloseTo(1000);
    expect(r.h * 1000).toBeCloseTo(1000);
    expect(r.x).toBeCloseTo(0.25);
    expect(r.y).toBeCloseTo(0);
  });
  it("zooms in and stays inside the picture", () => {
    const r = cropRect(1000, 1000, 16 / 9, 2, 1, 1);
    expect(r.x + r.w).toBeLessThanOrEqual(1.0001);
    expect(r.y + r.h).toBeLessThanOrEqual(1.0001);
    expect(r.w).toBeCloseTo(0.5);
  });
});

describe("presetPrompt", () => {
  it("fills in what the person typed", () => {
    expect(presetPrompt("remove", " the man on the left ")).toContain("Remove the man on the left from the photo");
    expect(presetPrompt("free", "make the sky stormy")).toBe("make the sky stormy");
  });
  it("needs nothing typed for restoring or removing the background", () => {
    expect(presetPrompt("restore", "")).toContain("Restore this old photo");
    expect(presetPrompt("background", "")).toContain("plain white");
  });
});
