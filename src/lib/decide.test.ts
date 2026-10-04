import { describe, expect, it } from "vitest";

import { decisionMarkdown, ranking, weightedTotals } from "./decide";
import type { Decision } from "./types";

const cell = (score: number) => ({ score, reason: "", sources: [] });
const d: Decision = {
  options: ["Air", "XPS"],
  criteria: [
    { name: "Battery", weight: 3 },
    { name: "Price", weight: 1 },
  ],
  scores: [
    [cell(9), cell(4)],
    [cell(6), cell(9)],
  ],
};

describe("compare & decide", () => {
  it("weights scores and re-ranks when weights change", () => {
    const t = weightedTotals(d);
    expect(t[0]).toBeCloseTo(7.75);
    expect(t[1]).toBeCloseTo(6.75);
    expect(ranking(t)).toEqual([0, 1]);
    // Price matters most now: the XPS wins.
    const t2 = weightedTotals(d, [1, 5]);
    expect(ranking(t2)).toEqual([1, 0]);
    // A criterion at 0 doesn't count; missing cells are skipped.
    expect(weightedTotals(d, [0, 1])).toEqual([4, 9]);
    expect(weightedTotals({ ...d, scores: [[null, null], [cell(5), null]] })).toEqual([0, 5]);
  });

  it("copies as a Markdown table", () => {
    const md = decisionMarkdown(d);
    expect(md.split("\n")[0]).toBe("| Criterion (weight) | Air | XPS |");
    expect(md).toContain("| Battery (3) | 9/10 | 6/10 |");
    expect(md).toContain("| **Total** | **7.8** | **6.8** |");
  });
});
