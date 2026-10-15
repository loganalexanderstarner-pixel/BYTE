import { describe, expect, it } from "vitest";

import { cleanList, recordingLine } from "./ScreenSettings";

describe("the never-look-at list", () => {
  it("trims, drops blanks and repeats, and accepts commas", () => {
    expect(cleanList("  Discord \n\nbitwarden\nDISCORD, bank")).toEqual(["Discord", "bitwarden", "bank"]);
    expect(cleanList("")).toEqual([]);
  });
});

describe("the replay's state in words", () => {
  it("says off, recording and paused", () => {
    expect(recordingLine(undefined, 0)).toBe("Not recording.");
    expect(recordingLine({ on: false, recording: false, paused: null, seconds: 30 }, 0)).toBe("Not recording.");
    expect(recordingLine({ on: true, recording: true, paused: null, seconds: 30 }, 12)).toContain("12 frames");
    expect(recordingLine({ on: true, recording: false, paused: "the screen is locked", seconds: 30 }, 0)).toBe("Paused: the screen is locked.");
  });
});
