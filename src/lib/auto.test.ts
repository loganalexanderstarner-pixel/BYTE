import { describe, expect, it } from "vitest";

import { canResume, emptyContract, feedLine, hasUnlimited, jobProblem, LEVELS, MESSAGE_OPTIONS, minutesText, REMEMBER_FIELD, splitList, splitRemember, STAGE_OPTIONS, statusText, STEP_OPTIONS, TIME_OPTIONS, type Job, type LiveView } from "./auto";

describe("autonomy levels", () => {
  it("has the three choices, each explained in one sentence", () => {
    expect(LEVELS.map((l) => l.value)).toEqual(["ask", "contract", "never"]);
    for (const l of LEVELS) expect(l.text.split(". ").length).toBeLessThanOrEqual(2);
    expect(LEVELS[2].text).toMatch(/payments are still refused/);
  });
});

describe("budgets", () => {
  it("every list of choices ends with Unlimited", () => {
    for (const o of [TIME_OPTIONS, STEP_OPTIONS, MESSAGE_OPTIONS, STAGE_OPTIONS]) expect(hasUnlimited(o)).toBe(true);
  });
  it("the default contract never deletes or spends", () => {
    const c = emptyContract();
    expect([c.neverDelete, c.neverSpend]).toEqual([true, true]);
  });
});

describe("lists and jobs", () => {
  it("splits names without repeats", () => {
    expect(splitList("Mom, mom; Dad\n")).toEqual(["Mom", "Dad"]);
    expect(splitList(" ")).toEqual([]);
  });
  it("names what is missing before a job starts", () => {
    const c = { ...emptyContract(), apps: ["notepad"] };
    const s = [{ text: "open it", tool: "pc" }];
    expect(jobProblem("", s, c)).toMatch(/what the job is/);
    expect(jobProblem("x", [], c)).toMatch(/Split/);
    expect(jobProblem("x", s, emptyContract())).toMatch(/program/);
    expect(jobProblem("x", s, c)).toBeNull();
  });
});

describe("cards", () => {
  it("hide the marker field and read the rule from it", () => {
    const r = splitRemember([{ label: "Step", value: "Press ctrl+s" }, { label: REMEMBER_FIELD, value: "Press ctrl+s in notepad" }]);
    expect(r.fields).toHaveLength(1);
    expect(r.rule).toBe("Press ctrl+s in notepad");
    expect(splitRemember([{ label: "Step", value: "x" }]).rule).toBe("");
  });
});

describe("live view text", () => {
  const v = (o: Partial<LiveView>): LiveView => ({ jobId: "", title: "", stagesTotal: 0, stagesDone: 0, stage: 0, stageText: "", next: "", now: "", status: "running", note: "", percent: 0, feed: [], checkIn: null, ...o });
  it("says what is happening", () => {
    expect(statusText(v({ now: "Click item 2" }))).toBe("Now: Click item 2");
    expect(statusText(v({ status: "waiting" }))).toBe("Waiting for your answer");
    expect(statusText(v({ status: "stopped", note: "you pressed Stop" }))).toBe("Stopped: you pressed Stop");
  });
  it("lists steps with a mark", () => {
    expect(feedLine({ app: "notepad", what: "Click item 1", ok: true, detail: "" })).toBe("✓ notepad: Click item 1");
    expect(feedLine({ app: "", what: "Open x", ok: false, detail: "no" })).toBe("✗ Open x (no)");
    expect(feedLine({ app: "a", what: "b", ok: null, detail: "" })).toBe("… a: b");
  });
  it("tells long times", () => {
    expect(minutesText(45_000)).toBe("45 s");
    expect(minutesText(10 * 60_000)).toBe("10 min");
    expect(minutesText(3 * 3_600_000)).toBe("3.0 h");
  });
  it("lets a stopped or failed job be resumed but not a finished one", () => {
    const j = (state: Job["state"]) => ({ state }) as Job;
    expect([canResume(j("paused")), canResume(j("stopped")), canResume(j("failed")), canResume(j("done")), canResume(j("running"))]).toEqual([true, true, true, false, false]);
  });
});

import { DISCLAIMER, liftBudget } from "./auto";

describe("resuming after a budget", () => {
  it("sets only the budget that ran out to Unlimited", () => {
    const c = emptyContract();
    expect(liftBudget(c, "the step budget (200) is used up")?.stepBudget).toBe("unlimited");
    expect(liftBudget(c, "the time budget (30 min) is used up")?.timeBudget).toBe("unlimited");
    expect(liftBudget(c, "you pressed Stop")).toBeNull();
  });
  it("has a notice that says what a job never does", () => {
    expect(DISCLAIMER.join(" ")).toMatch(/never deletes/);
    expect(DISCLAIMER.join(" ")).toMatch(/responsible/);
  });
});
