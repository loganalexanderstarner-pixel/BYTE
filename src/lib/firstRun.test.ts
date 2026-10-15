import { describe, expect, it } from "vitest";

import type { CapableModel } from "./control";
import { controlOffer, featurePatch, featuresFor, FIRST_RUN_FEATURES, offerBytes } from "./firstRun";

const m = (over: Partial<CapableModel> = {}): CapableModel => ({ id: "q", name: "Q", note: "", installedKey: null, downloadKey: "q:Q4", downloadBytes: 2.5e9, selected: false, ...over });

describe("first-run features", () => {
  it("changes nothing when nothing is ticked", () => {
    expect(featurePatch([], "q:Q4")).toEqual({});
  });
  it("turns on exactly what was ticked", () => {
    expect(featurePatch(["heyByte"], null)).toEqual({ wakeWord: true });
    expect(featurePatch(["screenAsk", "guidedHelp"], null)).toEqual({ screenAsk: true, guidedHelp: true });
    expect(featurePatch(["browser"], null)).toEqual({ browserVisible: true, browserLiveResearch: true });
  });
  it("sets the control model only together with PC control", () => {
    expect(featurePatch(["pcControl"], "q:Q4")).toEqual({ pcControl: true, controlModel: "q:Q4" });
    expect(featurePatch(["pcControl"], null)).toEqual({ pcControl: true });
    expect(featurePatch(["heyByte"], "q:Q4")).toEqual({ wakeWord: true });
  });
  it("offers the Windows-only features only on Windows, and none on a phone", () => {
    expect(featuresFor(false, false).map((f) => f.id)).toEqual(["heyByte", "browser"]);
    expect(featuresFor(true, false).length).toBe(FIRST_RUN_FEATURES.length);
    expect(featuresFor(true, true)).toEqual([]);
  });
  it("gives each feature a one-sentence explanation", () => {
    for (const f of FIRST_RUN_FEATURES) {
      expect(f.explain.length).toBeGreaterThan(30);
      expect((f.explain.match(/\. /g) ?? []).length, f.id).toBe(0);
    }
  });
  it("counts a download only when the model is not on disk", () => {
    expect(offerBytes(m())).toBe(2.5e9);
    expect(offerBytes(m({ installedKey: "q:Q4" }))).toBe(0);
    expect(offerBytes(null)).toBe(0);
    expect(controlOffer([])).toBeNull();
    expect(controlOffer([m()])?.id).toBe("q");
  });
});
