import { describe, expect, it } from "vitest";

import type { ImageModel } from "../../lib/types";
import { arrange } from "./ImageCatalog";

const m = (id: string, o: Partial<ImageModel>): ImageModel => ({
  id, name: id, family: "f", kind: "generate", tags: [], tagline: "", strengths: "", license: "x", commercialOk: true,
  quality: 50, steps: 4, community: false, mature: false, overheadGb: 2, downloads: 0, source: "a/b", variants: [], best: 0, totalBytes: 1, companions: [], ...o,
});
const base = { query: "", filter: null, sort: "best" as const, community: false, commercialOnly: false };

describe("arrange", () => {
  const list = [m("a", { quality: 60, tags: ["realism"] }), m("b", { quality: 90, steps: 30, totalBytes: 9 }), m("c", { quality: 70, community: true, commercialOk: false, tags: ["realism"] })];
  it("sorts best first and hides community models by default", () => {
    expect(arrange(list, base).map((x) => x.id)).toEqual(["b", "a"]);
  });
  it("shows community models when asked, and filters by style", () => {
    expect(arrange(list, { ...base, community: true, filter: "realism" }).map((x) => x.id)).toEqual(["c", "a"]);
  });
  it("hides adult models in kids mode even when community models are on", () => {
    const l = [...list, m("d", { community: true, mature: true })];
    expect(arrange(l, { ...base, community: true }).map((x) => x.id)).toContain("d");
    expect(arrange(l, { ...base, community: true, kids: true }).map((x) => x.id)).not.toContain("d");
  });
  it("can hide models that are not for commercial use", () => {
    expect(arrange(list, { ...base, community: true, commercialOnly: true }).map((x) => x.id)).toEqual(["b", "a"]);
  });
  it("sorts by speed and size and searches", () => {
    expect(arrange(list, { ...base, sort: "fastest" })[0].id).toBe("a");
    expect(arrange(list, { ...base, sort: "popular", community: true })[0].id).toBeDefined();
    expect(arrange(list, { ...base, sort: "smallest" })[1].id).toBe("b");
    expect(arrange(list, { ...base, query: "B" }).map((x) => x.id)).toEqual(["b"]);
  });
});
