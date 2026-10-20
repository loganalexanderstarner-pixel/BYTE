import { describe, expect, it } from "vitest";

import { fitPoem, quietestSide, regionFor, splitColumns, wantsLightText, wrapLines } from "./posterLayout";

const grid = (cols: number, rows: number, f: (c: number, r: number) => number) => Array.from({ length: cols * rows }, (_, i) => f(i % cols, Math.floor(i / cols)));

describe("quietestSide", () => {
  it("puts the words on the dark, even side", () => {
    const g = grid(16, 9, (c) => (c < 6 ? 25 : 200));
    expect(quietestSide(g, 16, 9)).toBe("left");
    expect(quietestSide(grid(16, 9, (c) => (c >= 10 ? 30 : 210)), 16, 9)).toBe("right");
  });
  it("avoids a busy side even if it is dark on average", () => {
    const g = grid(16, 9, (c, r) => (c < 6 ? ((c + r) % 2 ? 0 : 120) : 60));
    expect(quietestSide(g, 16, 9)).toBe("right");
  });
  it("chooses light text on dark ground and dark text on bright ground", () => {
    expect(wantsLightText(grid(8, 8, () => 30), 8, 8, "left")).toBe(true);
    expect(wantsLightText(grid(8, 8, () => 230), 8, 8, "left")).toBe(false);
  });
});

describe("wrapLines and columns", () => {
  const measure = (s: string) => s.length * 10;
  it("keeps the poem's line breaks and wraps only long lines", () => {
    expect(wrapLines("short\n\nthis line is much too long to fit", measure, 150)).toEqual(["short", "", "this line is", "  much too long", "  to fit"]);
  });
  it("drops blank lines at the ends", () => {
    expect(wrapLines("\n\na\n\n", measure, 100)).toEqual(["a"]);
  });
  it("splits columns at the blank line between stanzas", () => {
    const lines = ["a", "b", "c", "", "d", "e", "f"];
    expect(splitColumns(lines, 2)).toEqual([["a", "b", "c"], ["d", "e", "f"]]);
  });
});

describe("fitPoem", () => {
  const measure = (s: string, size: number) => s.length * size * 0.5;
  const poem = Array.from({ length: 4 }, (_, i) => `Line ${i} of the first verse goes here\nLine two of it goes on\nand a third line too`).join("\n\n");
  it("uses the biggest size that fits, with more columns only when it must", () => {
    const small = fitPoem("one line", regionFor("left", 1344, 768), measure)!;
    expect(small.columns.length).toBe(1);
    const f = fitPoem(poem, regionFor("left", 1344, 768), measure)!;
    expect(f.columns.length).toBeGreaterThanOrEqual(1);
    const tallest = Math.max(...f.columns.map((c) => c.length));
    expect(tallest * f.lineHeight).toBeLessThanOrEqual(768);
    for (const c of f.columns) for (const l of c) expect(measure(l, f.size)).toBeLessThanOrEqual(f.colWidth + 0.01);
  });
  it("keeps the poet's line breaks when any size allows it", () => {
    const f = fitPoem("The lighthouse keeps its lamp alight\nthough no one climbs the stair", { x: 0, y: 0, w: 600, h: 400 }, measure)!;
    expect(f.columns.flat().filter(Boolean)).toEqual(["The lighthouse keeps its lamp alight", "though no one climbs the stair"]);
    expect(f.size).toBeLessThan(64);
  });
  it("gives up when the text cannot fit even at the smallest size", () => {
    expect(fitPoem("x\n".repeat(400), { x: 0, y: 0, w: 300, h: 100 }, measure)).toBeNull();
  });
});
