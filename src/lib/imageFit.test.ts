import { describe, expect, it } from "vitest";

import { fitSentence, placementClass } from "./imageFit";
import type { ImageVariant } from "./types";

const v = (placement: ImageVariant["fit"]["placement"], overflowBytes = 0): ImageVariant => ({
  quant: "Q4_K", bits: 4.5, qualityCost: 4, sizeBytes: 6e9, files: [], installed: false, fit: { placement, needBytes: 8.5e9, overflowBytes },
});

describe("fitSentence", () => {
  it("says it fits", () => expect(fitSentence(v("vram"))).toMatch(/Fits in your graphics card/));
  it("says how much overflows into RAM", () => {
    const s = fitSentence(v("split", 3.2e9));
    expect(s).toMatch(/overflow into your RAM/);
    expect(s).toMatch(/3\.2 GB/);
  });
  it("says when it is too big and by how much", () => expect(fitSentence(v("toobig", 2e9))).toMatch(/2\.0 GB short/));
  it("colours by placement", () => {
    expect([placementClass("vram"), placementClass("split"), placementClass("ram"), placementClass("toobig")]).toEqual(["ok", "warn", "warn", "danger"]);
  });
});
