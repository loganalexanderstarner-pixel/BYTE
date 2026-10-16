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

import { batteryBadgeText, smallerModel } from "./found";
import type { ModelStatus } from "./types";

const mdl = (name: string, key: string, size: number, installed: boolean, role = "chat") =>
  ({ name, role, variants: [{ key, sizeBytes: size, installed }] }) as unknown as ModelStatus;

describe("laptop mode", () => {
  const list = [mdl("Big", "big:q", 10e9, true), mdl("Mid", "mid:q", 6e9, true), mdl("Small", "small:q", 2e9, true), mdl("Away", "away:q", 1e9, false), mdl("Voice", "v:q", 1.5e9, true, "voice")];
  it("suggests the largest downloaded chat model that is clearly smaller", () => {
    expect(smallerModel(list, "big:q")).toEqual({ key: "mid:q", name: "Mid" });
    expect(smallerModel(list, "mid:q")).toEqual({ key: "small:q", name: "Small" });
  });
  it("suggests nothing when there is nothing smaller downloaded or the active model is unknown", () => {
    expect(smallerModel(list, "small:q")).toBeNull();
    expect(smallerModel(list, null)).toBeNull();
    expect(smallerModel(list, "gone:q")).toBeNull();
  });
  it("shows a badge only on battery with the mode on", () => {
    expect(batteryBadgeText({ onBattery: true, laptopMode: true, paused: ["Hey BYTE listening", "the screen replay"] })).toBe("On battery: Hey BYTE listening and the screen replay paused");
    expect(batteryBadgeText({ onBattery: true, laptopMode: true, paused: [] })).toBe("On battery");
    expect(batteryBadgeText({ onBattery: false, laptopMode: true, paused: [] })).toBe("");
    expect(batteryBadgeText({ onBattery: true, laptopMode: false, paused: [] })).toBe("");
  });
});

import { OWN_EDGE_WARNING, UNTIL_BACK_CHOICES } from "./found";

describe("browser research choices", () => {
  it("has an Unlimited choice and starts at Off", () => {
    expect(UNTIL_BACK_CHOICES[0].value).toBe("off");
    expect(UNTIL_BACK_CHOICES.some((c) => c.value === "unlimited")).toBe(true);
    expect(new Set(UNTIL_BACK_CHOICES.map((c) => c.value)).size).toBe(UNTIL_BACK_CHOICES.length);
  });
  it("warns plainly about the own-profile choice", () => {
    expect(OWN_EDGE_WARNING).toMatch(/everyday profile/);
    expect(OWN_EDGE_WARNING).toMatch(/never types passwords/);
  });
});
