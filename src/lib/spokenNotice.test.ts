import { describe, expect, it } from "vitest";

import { downloadedLine, jobDoneLine, shouldSpeakNotice } from "./spokenNotice";

const ok = { enabled: true, speechOn: true, game: false, meeting: false };

describe("shouldSpeakNotice", () => {
  it("speaks when it is on, speech is on and nothing is in the way", () => {
    expect(shouldSpeakNotice(ok)).toBe(true);
  });
  it("is off by the setting or when speech is off", () => {
    expect(shouldSpeakNotice({ ...ok, enabled: false })).toBe(false);
    expect(shouldSpeakNotice({ ...ok, speechOn: false })).toBe(false);
  });
  it("stays quiet in a game or a call, and when that cannot be told", () => {
    expect(shouldSpeakNotice({ ...ok, game: true })).toBe(false);
    expect(shouldSpeakNotice({ ...ok, meeting: true })).toBe(false);
    expect(shouldSpeakNotice({ ...ok, game: null })).toBe(false);
    expect(shouldSpeakNotice({ ...ok, meeting: null })).toBe(false);
  });
});

describe("the lines", () => {
  it("are plain sentences", () => {
    expect(downloadedLine("Qwen3 14B")).toBe("Your Qwen3 14B model finished downloading.");
    expect(downloadedLine("")).toBe("Your model finished downloading.");
    expect(jobDoneLine("the speed test")).toBe("The speed test finished.");
  });
});
