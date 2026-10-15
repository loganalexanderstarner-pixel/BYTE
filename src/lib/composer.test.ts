import { describe, expect, it } from "vitest";

import { IDEA_TONES, TONES, tidy } from "./composer";

describe("composer", () => {
  it("drops lead-ins and quotes models add", () => {
    expect(tidy("Here's a friendlier version:\nHey! Running late, sorry")).toBe("Hey! Running late, sorry");
    expect(tidy("“On my way!”")).toBe("On my way!");
    expect(tidy("  Plain text  ")).toBe("Plain text");
  });
  it("offers the tones the owner asked for", () => {
    expect(TONES.map((t) => t.label)).toEqual(["Formal", "Friendly", "Fun", "Sympathetic"]);
    expect(IDEA_TONES).toHaveLength(3);
  });
});
