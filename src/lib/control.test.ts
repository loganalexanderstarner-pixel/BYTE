import { describe, expect, it } from "vitest";

import { addApp, newStep, routineProblem, sizeText, stepLabel, type Routine } from "./control";

const base: Routine = { id: "", phrase: "game time", steps: [{ type: "openApp", name: "steam" }], schedule: "", enabled: true };

describe("routines", () => {
  it("need a phrase and a step", () => {
    expect(routineProblem(base)).toBeNull();
    expect(routineProblem({ ...base, phrase: " a " })).toMatch(/phrase/);
    expect(routineProblem({ ...base, steps: [] })).toMatch(/at least one/);
  });
  it("name what each step needs", () => {
    expect(routineProblem({ ...base, steps: [newStep("openApp")] })).toMatch(/Step 1 needs a name/);
    expect(routineProblem({ ...base, steps: [newStep("openUrl")] })).toMatch(/web address/);
    expect(routineProblem({ ...base, steps: [{ type: "openUrl", url: "https://example.com" }] })).toBeNull();
    expect(routineProblem({ ...base, steps: [{ type: "openUrl", url: "file:///c:/x" }] })).toMatch(/web address/);
    expect(routineProblem({ ...base, steps: [{ type: "volume", percent: 120 }] })).toMatch(/0 to 100/);
  });
  it("read as short lines", () => {
    expect(stepLabel({ type: "volume", percent: 30 })).toBe("Volume to 30%");
    expect(stepLabel({ type: "doNotDisturb", on: true })).toBe("Turn notifications off");
    expect(stepLabel(newStep("closeApp"))).toBe("Close …");
  });
});

describe("allowed programs", () => {
  it("are stored plainly and once", () => {
    expect(addApp([], " Notepad.EXE ")).toEqual(["notepad"]);
    expect(addApp(["notepad"], "notepad")).toEqual(["notepad"]);
    expect(addApp([], "a;b")).toEqual([]);
    expect(addApp([], "  ")).toEqual([]);
  });
});

describe("sizes", () => {
  it("are in gigabytes", () => {
    expect(sizeText(2.5 * 1024 ** 3)).toBe("2.5 GB");
    expect(sizeText(20 * 1024 ** 3)).toBe("20 GB");
  });
});

import { bannerText } from "./control";

describe("banner", () => {
  it("names the goal, shortened", () => {
    expect(bannerText("")).toBe("BYTE is controlling your PC");
    expect(bannerText("  open   notepad ")).toBe("BYTE is controlling your PC: open notepad");
    expect(bannerText("x".repeat(100)).endsWith("…")).toBe(true);
  });
});

import { guideStatus } from "./control";

describe("guide card status", () => {
  it("says what to do while showing and doing", () => {
    expect(guideStatus("offer", "")).toBe("");
    expect(guideStatus("showing", "")).toMatch(/F8/);
    expect(guideStatus("doing", "")).toMatch(/Stop/);
    expect(guideStatus("done", "Done, 3 steps.")).toBe("Done, 3 steps.");
    expect(guideStatus("done", "")).toBe("Finished.");
  });
});
