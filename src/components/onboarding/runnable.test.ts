import { describe, expect, it } from "vitest";

import type { ModelStatus } from "../../lib/types";
import { runnable } from "./Onboarding";

const model = (id: string, bits: number, quality: number, size: number, tags: string[] = [], spill = false) =>
  ({
    id,
    tags,
    role: "chat",
    best: `${id}:q`,
    variants: [{ key: `${id}:q`, bits, quality, sizeBytes: size, fit: spill ? { cpuMoeLayers: 20 } : {} }],
  }) as unknown as ModelStatus;

describe("runnable", () => {
  it("lists good-quality versions before squeezed ones", () => {
    const list = runnable([model("big", 2.5, 95, 8e9), model("mid", 6.5, 80, 7e9), model("small", 4.5, 70, 3e9)]);
    expect(list.map((x) => x.model.id)).toEqual(["mid", "small", "big"]);
  });
  it("puts community remixes last", () => {
    const list = runnable([model("remix", 6.5, 99, 7e9, ["community", "uncensored"]), model("big", 2.5, 95, 8e9), model("mid", 6.5, 80, 7e9)]);
    expect(list.map((x) => x.model.id)).toEqual(["mid", "big", "remix"]);
  });
  it("skips models with no version that fits", () => {
    const none = { ...model("x", 6, 90, 1e9), best: null } as unknown as ModelStatus;
    expect(runnable([none])).toEqual([]);
  });
  describe("on a PC with its own graphics memory", () => {
    // A model that spills onto the CPU is the best on paper and several times slower in use.
    const spilled = model("spilled", 6.5, 90, 23e9, [], true);
    const onCard = model("oncard", 6.5, 80, 7e9);
    const squeezed = model("squeezed", 3, 85, 11e9);
    it("lists models that stay on the card before ones that spill", () => {
      const list = runnable([spilled, onCard, squeezed], { pick: null });
      expect(list.map((x) => x.model.id)).toEqual(["oncard", "spilled", "squeezed"]);
    });
    it("puts BYTE's pick first, so the first card is what Continue downloads", () => {
      const list = runnable([spilled, onCard, squeezed], { pick: "squeezed:q" });
      expect(list.map((x) => x.model.id)).toEqual(["squeezed", "oncard", "spilled"]);
    });
    it("ignores a pick that is not on the list", () => {
      const list = runnable([onCard, squeezed], { pick: "gone:q" });
      expect(list.map((x) => x.model.id)).toEqual(["oncard", "squeezed"]);
    });
    it("leaves a Mac's order alone", () => {
      const list = runnable([spilled, onCard, squeezed]);
      expect(list.map((x) => x.model.id)).toEqual(["spilled", "oncard", "squeezed"]);
    });
  });
});
