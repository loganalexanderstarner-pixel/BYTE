import { describe, expect, it } from "vitest";

import { clipAge, clipPreview } from "./clips";

describe("clipboard history", () => {
  it("shows one tidy line", () => {
    expect(clipPreview("  Hello\n\n  world\t!  ")).toBe("Hello world !");
    const long = clipPreview("x".repeat(400));
    expect(long.length).toBe(280);
    expect(long.endsWith("…")).toBe(true);
  });
  it("says how long ago", () => {
    const now = 1_000_000_000_000;
    expect(clipAge(now - 5_000, now)).toBe("just now");
    expect(clipAge(now - 5 * 60_000, now)).toBe("5 min ago");
    expect(clipAge(now - 3 * 3_600_000, now)).toBe("3 h ago");
    expect(clipAge(now - 26 * 3_600_000, now)).toBe("yesterday");
    expect(clipAge(now - 4 * 86_400_000, now)).toBe("4 days ago");
  });
});
