import { describe, expect, it } from "vitest";

import { bodyOf, budgetSummary, isImage, jobState, listOf, readOutline, writeOutline } from "./cloudDocs";

describe("cloud replies", () => {
  it("finds lists however they're wrapped", () => {
    expect(listOf([{ id: 1 }])).toHaveLength(1);
    expect(listOf({ documents: [{ id: 1 }, { id: 2 }] })).toHaveLength(2);
    expect(listOf({ nothing: true })).toEqual([]);
    expect(listOf(null)).toEqual([]);
  });

  it("reads job progress and the approval step", () => {
    expect(jobState({ status: "running", progress: 40, phase: "Writing section 2" })).toEqual({ phase: "working", progress: 0.4, label: "Writing section 2" });
    expect(jobState({ status: "awaiting_approval" }).phase).toBe("approval");
    expect(jobState({ status: "outline_ready" }).phase).toBe("approval");
    expect(jobState({ status: "done", document_id: 9 })).toMatchObject({ phase: "done", documentId: "9" });
    expect(jobState({ status: "failed", error: "out of budget" })).toMatchObject({ phase: "failed", label: "out of budget" });
  });

  it("edits an outline and sends it back in the server's shape", () => {
    const objects = readOutline({ outline: [{ title: "Intro", bullets: ["a"] }, { heading: "Costs" }] });
    expect(objects.map((o) => o.title)).toEqual(["Intro", "Costs"]);
    objects[1].title = "Budget";
    expect(writeOutline(objects)).toEqual([{ title: "Intro", bullets: ["a"] }, { heading: "Budget" }]);
    const strings = readOutline(["One", "Two"]);
    strings.push({ title: "Three", raw: null, key: "new" });
    expect(writeOutline(strings)).toEqual(["One", "Two", "Three"]);
  });

  it("spots images", () => {
    expect(isImage({ content_type: "image/jpeg" })).toBe(true);
    expect(isImage({ filename: "IMG_1.HEIC" })).toBe(true);
    expect(isImage({ filename: "report.pdf" })).toBe(false);
  });

  it("reads item text under any common name", () => {
    expect(bodyOf({ content: "a" })).toBe("a");
    expect(bodyOf({ prompt: "b" })).toBe("b");
    expect(bodyOf({ id: 1 })).toBe("");
  });
});

describe("budgets", () => {
  it("shows what's left in the chat box and everything in the tooltip", () => {
    const b = budgetSummary({ documents: { used: 1, remaining: 4 }, extended_per_day: 5 });
    expect(b?.short).toBe("4 left");
    expect(b?.detail).toContain("documents · remaining: 4");
    expect(b?.detail).toContain("extended per day: 5");
    expect(budgetSummary({ daily_tokens: 100000 })?.short).toBe("daily tokens: 100000");
    expect(budgetSummary(null)).toBeNull();
    // A null value is not a row: no "null left".
    expect(budgetSummary({ extended_per_day: { used: 1, remaining: null } })?.short).toBe("extended per day · used: 1");
    expect(budgetSummary({ extended_per_day: { remaining: null } })).toBeNull();
  });
});
