import { describe, expect, it } from "vitest";

import { blankJob, daysLeft, deadlineText, groupJobs } from "./jobs";

const today = new Date(2026, 8, 29); // 29 Sep 2026

describe("deadlines", () => {
  it("counts days and says it plainly", () => {
    expect(daysLeft("2026-10-01", today)).toBe(2);
    expect(deadlineText("2026-09-29", today)).toBe("apply today");
    expect(deadlineText("2026-09-30", today)).toBe("apply by tomorrow");
    expect(deadlineText("2026-10-05", today)).toBe("6 days left");
    expect(deadlineText("2026-12-01", today)).toBe("apply by 2026-12-01");
    expect(deadlineText("2026-09-01", today)).toBe("deadline passed");
    expect(deadlineText("", today)).toBe("");
  });
});

describe("groups", () => {
  it("follows the pipeline order and drops empty groups", () => {
    const j = (status: string, role: string) => ({ ...blankJob(), status, role });
    const g = groupJobs([j("offer", "A"), j("saved", "B"), j("weird", "C"), j("applied", "D")]);
    expect(g.map(([s, l]) => [s, l.map((x) => x.role)])).toEqual([
      ["saved", ["B", "C"]],
      ["applied", ["D"]],
      ["offer", ["A"]],
    ]);
  });
});
