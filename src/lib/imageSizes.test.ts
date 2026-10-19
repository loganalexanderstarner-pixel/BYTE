import { describe, expect, it } from "vitest";

import { sizeOf, SIZES, took } from "./imageSizes";

describe("image sizes", () => {
  it("every size is a multiple of 16", () => {
    for (const s of SIZES) expect([s.w % 16, s.h % 16]).toEqual([0, 0]);
  });
  it("falls back to portrait for an unknown id", () => expect(sizeOf("nope")).toEqual({ w: 896, h: 1152 }));
  it("words a wait", () => {
    expect(took(62.4)).toBe("62 s");
    expect(took(200)).toBe("3 min 20 s");
  });
});
