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

import { whatsNewParts } from "./found";

describe("what's new card", () => {
  const notes = "# Phase 12 improvements — Texting from BYTE, and a Messages inbox (v0.12.6)\n\n**BYTE v0.12.6** intro.\n\n## New in this version\n- **A:** one\n- **B:** two\n\n## What BYTE can do in this version\nEverything.\n\n## Not in this version yet\n- later\n";
  it("keeps the new things and drops the rest", () => {
    const p = whatsNewParts(notes);
    expect(p.title).toBe("Texting from BYTE, and a Messages inbox");
    expect(p.body).toBe("- **A:** one\n- **B:** two");
  });
  it("falls back to the text after the title when there is no 'New' heading", () => {
    const p = whatsNewParts("# Release (v1.0.0)\n\nSomething happened.\n");
    expect(p.title).toBe("Release");
    expect(p.body).toBe("Something happened.");
    expect(whatsNewParts("").title).toBe("What's new");
  });
});
