import { describe, expect, it } from "vitest";

import { ASK_WHERE, READ_SPEEDS, wakePhraseProblem, BREAK_MINUTES, FOCUS_MINUTES, MINUTES, breakLabel, defaultHelperKeys, helperKeys, helperRows, minutesLabel } from "./helpers";

describe("helper settings rows", () => {
  it("has a row for each helper with its own switch, and Windows-only ones hide elsewhere", () => {
    expect(helperRows(true).map((r) => r.key)).toEqual([
      "helperRegion", "helperExplain", "helperDialogs", "helperSmartPaste", "helperDictate", "helperPage", "helperMeeting", "helperFocus",
      "helperTidy", "helperRecipes", "helperHa", "helperGame", "helperNav", "helperRead", "helperCaptions",
      "helperPagetools", "helperEmail", "helperProduct", "helperForm",
    ]);
    expect(helperRows(false).map((r) => r.key)).toEqual(["helperExplain", "helperSmartPaste", "helperTidy", "helperHa", "helperRead"]);
  });

  it("offers Local first and as the only default, then Auto and Cloud", () => {
    expect(ASK_WHERE.map((o) => o.id)).toEqual(["local", "auto", "cloud"]);
  });

  it("uses the saved keys, else the defaults for the system", () => {
    expect(helperKeys({}, true)).toEqual(defaultHelperKeys(true));
    expect(helperKeys({ helperRegionKeys: "Control+Shift+KeyY" }, true).helperRegionKeys).toBe("Control+Shift+KeyY");
    expect(defaultHelperKeys(false).helperRegionKeys).toBe("Alt+Super+KeyR");
    expect(defaultHelperKeys(true).helperRegionKeys).not.toBe(defaultHelperKeys(true).helperExplainKeys);
  });

  it("no row promises to send anything to the cloud on its own", () => {
    for (const r of helperRows(true)) expect(r.text.toLowerCase()).not.toMatch(/sends? [^.]*cloud/);
  });

  it("every count has a No limit choice", () => {
    expect(MINUTES).toContain(0);
    expect(minutesLabel(0)).toBe("No limit");
    expect(minutesLabel(120)).toBe("2 hours");
    expect(minutesLabel(60)).toBe("1 hour");
    expect(minutesLabel(45)).toBe("45 minutes");
    expect(FOCUS_MINUTES).toContain(0);
    expect(BREAK_MINUTES).toContain(0);
    expect(breakLabel(0)).toBe("No reminders");
    expect(breakLabel(50)).toBe("50 minutes");
  });

  it("checks a custom wake phrase the way the app does", () => {
    expect(wakePhraseProblem("")).toBeNull();
    expect(wakePhraseProblem("Hey Jarvis")).toBeNull();
    expect(wakePhraseProblem("hey")).toContain("too short");
    expect(wakePhraseProblem("okay")).not.toBeNull();
    expect(wakePhraseProblem("one two three four")).toContain("one to three");
    expect(wakePhraseProblem("hey 123")).toContain("letters");
    expect(wakePhraseProblem("a".repeat(31))).toContain("too long");
    expect(READ_SPEEDS.map((s) => s.id)).toEqual(["slow", "normal", "fast"]);
  });
});
