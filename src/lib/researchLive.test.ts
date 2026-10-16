import { describe, expect, it } from "vitest";

import { elapsedText, isImBack, liveLine } from "./researchLive";

describe("saying you are back", () => {
  it("accepts the ways people say it", () => {
    for (const t of ["I'm back", "im back", "I am back.", "Hey BYTE, I'm back", "ok I'm back now", "I’m back!", "stop researching", "okay i'm back", "I'm back byte"]) expect(isImBack(t), t).toBe(true);
  });
  it("does not end a job because of a longer sentence that mentions it", () => {
    for (const t of ["I'm back at the airport, find me flights", "tell me when I'm back", "research e-bikes", "back", "stop"]) expect(isImBack(t), t).toBe(false);
  });
});

describe("the live card", () => {
  it("says where it works, how many pages, how long and the limit", () => {
    const base = { active: true, pages: 1, elapsedSecs: 125, limitSecs: null, standby: false, tabs: true };
    expect(liveLine(base)).toBe("BYTE is researching in Edge tabs you can watch: 1 page so far, 2 minutes, no time limit.");
    expect(liveLine({ ...base, pages: 7, limitSecs: 3600, tabs: false })).toBe("BYTE is researching in the background: 7 pages so far, 2 minutes, up to 1 h 0 min.");
    expect(liveLine({ ...base, standby: true })).toContain("standing by");
    expect(elapsedText(30)).toBe("under a minute");
  });
});
