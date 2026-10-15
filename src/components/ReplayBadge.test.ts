import { describe, expect, it } from "vitest";

import { badgeText } from "./ReplayBadge";

describe("the replay badge", () => {
  it("is hidden when off and says what is kept when recording", () => {
    expect(badgeText(undefined)).toBeNull();
    expect(badgeText({ on: false, recording: false, paused: null, seconds: 30 })).toBeNull();
    expect(badgeText({ on: true, recording: true, paused: null, seconds: 30 })).toBe("BYTE is keeping the last 30 seconds of your screen in memory");
    expect(badgeText({ on: true, recording: false, paused: "the screen is locked", seconds: 30 })).toContain("paused (the screen is locked)");
  });
});
