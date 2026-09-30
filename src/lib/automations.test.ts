import { describe, expect, it } from "vitest";

import { blankAutomation, failedAt, moveStep, newStep, problem, runStatus, stepLabel } from "./automations";
import type { StepRun } from "./types";

const run = (...statuses: StepRun["status"][]): StepRun[] => statuses.map((status, i) => ({ label: `s${i}`, status, detail: "" }));

describe("automations", () => {
  it("labels steps like Rust does", () => {
    expect(stepLabel({ type: "ask", prompt: "find AI news" })).toBe("Ask BYTE: find AI news");
    expect(stepLabel({ type: "addTask", title: "{previous}" })).toBe("Add it to your to-do list");
    expect(stepLabel({ type: "addTask", title: "Buy milk" })).toBe("Add “Buy milk” to your to-do list");
    expect(stepLabel({ type: "shortcut", name: "Log" })).toBe("Run your shortcut “Log”");
    expect(stepLabel(newStep("briefing"))).toBe("Write your daily briefing");
  });

  it("says what's missing before saving", () => {
    const a = { ...blankAutomation(), name: "News" };
    expect(problem(a)).toBe("Step 1 needs a question.");
    expect(problem({ ...a, steps: [{ type: "ask", prompt: "find news" }] })).toBeNull();
    expect(problem({ ...a, name: " " })).toBe("Give it a name.");
    expect(problem({ ...a, steps: [] })).toBe("Add at least one step.");
    expect(problem({ ...a, steps: [newStep("saveFile", "x"), { type: "ask", prompt: "x" }] })).toMatch(/first step/);
    expect(problem({ ...a, steps: [{ type: "addTask", title: "Water plants" }] })).toBeNull();
    expect(problem({ ...a, steps: [newStep("briefing"), newStep("shortcut")] })).toBe("Step 2 needs the shortcut's name.");
  });

  it("moves steps", () => {
    expect(moveStep([1, 2, 3], 0, 1)).toEqual([2, 1, 3]);
    expect(moveStep([1, 2, 3], 0, -1)).toEqual([1, 2, 3]);
    expect(moveStep([1, 2, 3], 2, 1)).toEqual([1, 2, 3]);
  });

  it("reads a run's state", () => {
    expect(runStatus(run("done", "running", "waiting"), false)).toBe("Step 2 of 3");
    expect(runStatus(run("done", "failed", "waiting"), true)).toBe("Stopped at step 2");
    expect(runStatus(run("done", "done"), true)).toBe("Done");
    expect(runStatus(run("waiting", "waiting"), false)).toBe("Starting…");
    expect(failedAt(run("done", "failed"))).toBe(1);
    expect(failedAt(run("done"))).toBeNull();
  });
});
