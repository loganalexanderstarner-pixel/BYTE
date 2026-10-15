import { describe, expect, it } from "vitest";

import { crashCardText, safeBannerText } from "./found";

describe("safe mode and crash wording", () => {
  it("says settings were not changed, and counts what was held off", () => {
    expect(safeBannerText({ active: true, heldOff: [] })).toMatch(/not changed/);
    expect(safeBannerText({ active: true, heldOff: ["wakeWord"] })).toMatch(/1 feature was on/);
    expect(safeBannerText({ active: true, heldOff: ["a", "b"] })).toMatch(/2 features were on/);
  });
  it("names the version that stopped when it is known", () => {
    expect(crashCardText({ version: "0.12.6", panic: null })).toContain("version 0.12.6");
    expect(crashCardText({ version: "", panic: null })).not.toContain("version");
  });
});
