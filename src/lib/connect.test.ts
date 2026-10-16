import { describe, expect, it } from "vitest";

import { blankJob, countLabel, defaultWhen, describeWhen, groupChannels, jobProblem, KEEP, PER_HOUR, toggleChannel, wasSkipped, WIZARDS, WHEN_CHOICES, EMAIL_STEPS } from "./connect";

describe("connect helpers", () => {
  it("every count has an Unlimited choice", () => {
    expect(PER_HOUR).toContain(0);
    expect(KEEP).toContain(0);
    expect(countLabel(0, "hour")).toBe("Unlimited");
    expect(countLabel(20, "hour")).toBe("20 per hour");
  });

  it("a new job is off and checks its own form", () => {
    const j = blankJob();
    expect(j.enabled).toBe(false);
    expect(jobProblem(j)).toMatch(/name/);
    expect(jobProblem({ ...j, name: "News" })).toMatch(/few words/);
    expect(jobProblem({ ...j, name: "News", instruction: "Summarise Rust news" })).toBeNull();
    expect(jobProblem({ ...j, name: "x", template: "digest" })).toBeNull();
    expect(jobProblem({ ...j, name: "x", instruction: "y", when: { type: "everyHours", hours: 0 } })).toMatch(/hours/);
    expect(jobProblem({ ...j, name: "x", instruction: "y", when: { type: "folder", path: " " } })).toMatch(/folder/);
  });

  it("every choice has a default and a description", () => {
    for (const c of WHEN_CHOICES) {
      expect(describeWhen(defaultWhen(c.id)).length, c.id).toBeGreaterThan(5);
    }
  });

  it("ticks and unticks channels, grouped by server", () => {
    const a = { id: "1", name: "#a" };
    expect(toggleChannel([], a)).toEqual([a]);
    expect(toggleChannel([a], a)).toEqual([]);
    const g = groupChannels([
      { id: "1", name: "#a", group: "S1" },
      { id: "2", name: "#b", group: "S2" },
      { id: "3", name: "#c", group: "S1" },
    ]);
    expect(g.map((x) => [x.group, x.channels.length])).toEqual([["S1", 2], ["S2", 1]]);
  });

  it("the setup steps are numbered, explain the Discord intent and refuse personal accounts", () => {
    expect(WIZARDS.discord.steps.length).toBeGreaterThanOrEqual(4);
    expect(WIZARDS.discord.steps.map((s) => s.text).join(" ")).toMatch(/Message Content/);
    expect(WIZARDS.discord.note).toMatch(/never logs in to a personal Discord account/);
    expect(WIZARDS.telegram.steps[0].link?.url).toMatch(/^https:\/\/t\.me\//);
    expect(EMAIL_STEPS.map((s) => s.text).join(" ")).toMatch(/app password/);
  });

  it("skipped runs are recognised", () => {
    expect(wasSkipped({ id: 1, jobId: 1, started: 0, finished: 0, ok: false, summary: "Skipped: all jobs are paused.", chat: null, trigger: "schedule" })).toBe(true);
  });
});
