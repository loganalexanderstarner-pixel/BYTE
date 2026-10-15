import { describe, expect, it } from "vitest";

import { heyByteUsesCloud } from "./heyByte";

describe("heyByteUsesCloud", () => {
  it("never uses the cloud without a connected account", () => {
    for (const e of ["auto", "cloud", "local", undefined]) expect(heyByteUsesCloud(e, false, true, false)).toBe(false);
  });
  it("follows an explicit choice", () => {
    expect(heyByteUsesCloud("cloud", true, false, true)).toBe(true);
    expect(heyByteUsesCloud("local", true, true, false)).toBe(false);
  });
  it("automatic: the cloud while gaming or with no local model, this PC otherwise", () => {
    expect(heyByteUsesCloud("auto", true, true, true)).toBe(true);
    expect(heyByteUsesCloud("auto", true, false, false)).toBe(true);
    expect(heyByteUsesCloud("auto", true, false, true)).toBe(false);
    expect(heyByteUsesCloud("auto", true, null, true)).toBe(false);
    expect(heyByteUsesCloud(undefined, true, true, true)).toBe(true);
  });
});
