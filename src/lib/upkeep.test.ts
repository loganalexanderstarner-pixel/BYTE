import { describe, expect, it } from "vitest";

import { sizeText, treemap, usedPercent } from "./upkeep";

describe("sizes", () => {
  it("reads like Finder", () => {
    expect(sizeText(1_234_000_000)).toBe("1.2 GB");
    expect(sizeText(52_000_000)).toBe("52 MB");
    expect(sizeText(2_500_000_000_000)).toBe("2.5 TB");
    expect(sizeText(900)).toBe("900 bytes");
  });
  it("knows how full the disk is", () => {
    expect(usedPercent(1000, 250)).toBe(75);
    expect(usedPercent(0, 0)).toBeNull();
  });
});

describe("treemap", () => {
  const area = (r: { w: number; h: number }) => r.w * r.h;

  it("gives each value its share of the box, inside the box, without overlaps", () => {
    const values = [60, 25, 10, 5];
    const rects = treemap(values, 400, 200);
    const total = 400 * 200;
    rects.forEach((r, i) => {
      expect(area(r)).toBeCloseTo((values[i] / 100) * total, 3);
      expect(r.x).toBeGreaterThanOrEqual(-1e-9);
      expect(r.y).toBeGreaterThanOrEqual(-1e-9);
      expect(r.x + r.w).toBeLessThanOrEqual(400 + 1e-6);
      expect(r.y + r.h).toBeLessThanOrEqual(200 + 1e-6);
    });
    for (let a = 0; a < rects.length; a++)
      for (let b = a + 1; b < rects.length; b++) {
        const A = rects[a];
        const B = rects[b];
        const overlapW = Math.min(A.x + A.w, B.x + B.w) - Math.max(A.x, B.x);
        const overlapH = Math.min(A.y + A.h, B.y + B.h) - Math.max(A.y, B.y);
        expect(overlapW <= 1e-6 || overlapH <= 1e-6).toBe(true);
      }
  });

  it("keeps the input order and skips empty values", () => {
    const rects = treemap([0, 10, 30], 100, 100);
    expect(area(rects[0])).toBe(0);
    expect(area(rects[2])).toBeCloseTo(7500, 3);
    expect(treemap([], 100, 100)).toEqual([]);
    expect(treemap([5], 0, 100)[0].w).toBe(0);
  });

  it("stays close to square for equal values", () => {
    const rects = treemap([1, 1, 1, 1], 200, 200);
    for (const r of rects) expect(Math.max(r.w / r.h, r.h / r.w)).toBeLessThan(1.01);
  });
});
