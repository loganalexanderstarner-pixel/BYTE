import { describe, expect, it } from "vitest";

import type { ImageModel } from "../../lib/types";
import { arrange, goodAt } from "./ImageCatalog";

const m = (id: string, o: Partial<ImageModel>): ImageModel => ({
  id, name: id, family: "f", kind: "generate", tags: [], looks: [], tagline: "", strengths: "", license: "x", commercialOk: true,
  quality: 50, steps: 4, community: false, mature: false, overheadGb: 2, downloads: 0, source: "a/b", variants: [], best: 0, totalBytes: 1, companions: [], ...o,
});
const base = { query: "", filter: null, sort: "best" as const, tab: "official" as const, commercialOnly: false };

describe("arrange", () => {
  const list = [m("a", { quality: 60, tags: ["realism"] }), m("b", { quality: 90, steps: 30, totalBytes: 9 }), m("c", { quality: 70, community: true, commercialOk: false, tags: ["realism"] })];
  it("sorts best first and keeps community models on their own tab", () => {
    expect(arrange(list, base).map((x) => x.id)).toEqual(["b", "a"]);
  });
  it("shows only community models on the Community tab, and filters by style", () => {
    expect(arrange(list, { ...base, tab: "community" as const }).map((x) => x.id)).toEqual(["c"]);
    expect(arrange(list, { ...base, tab: "community" as const, filter: "realism" }).map((x) => x.id)).toEqual(["c"]);
    expect(arrange(list, { ...base, filter: "realism" }).map((x) => x.id)).toEqual(["a"]);
  });
  it("narrows Realism to a real-life look", () => {
    const l = [m("p", { tags: ["realism"], looks: ["portrait"] }), m("q", { tags: ["realism"], looks: ["food"] }), m("r", { tags: ["fast"], looks: ["food"] })];
    expect(arrange(l, { ...base, filter: "realism", look: "food" }).map((x) => x.id)).toEqual(["q"]);
    expect(arrange(l, { ...base, filter: "realism" }).map((x) => x.id).sort()).toEqual(["p", "q"]);
    expect(arrange(l, { ...base, look: "food" }).length).toBe(3); // a look alone does nothing without Realism
  });
  it("hides adult models in kids mode even when community models are on", () => {
    const l = [...list, m("d", { community: true, mature: true })];
    expect(arrange(l, { ...base, tab: "community" as const }).map((x) => x.id)).toContain("d");
    expect(arrange(l, { ...base, tab: "community" as const, kids: true }).map((x) => x.id)).not.toContain("d");
  });
  it("keeps upscalers on their own tab and out of the model tabs", () => {
    const l = [...list, m("u", { kind: "upscale", community: true, tags: ["photo"] })];
    expect(arrange(l, { ...base, tab: "upscale" as const }).map((x) => x.id)).toEqual(["u"]);
    expect(arrange(l, { ...base, tab: "community" as const }).map((x) => x.id)).toEqual(["c"]);
    expect(arrange(l, { ...base, tab: "upscale" as const, filter: "photo" }).length).toBe(1);
    expect(arrange(l, { ...base, tab: "upscale" as const, filter: "anime" }).length).toBe(0);
  });
  it("can hide models that are not for commercial use", () => {
    expect(arrange(list, { ...base, tab: "community" as const, commercialOnly: true }).map((x) => x.id)).toEqual([]);
  });
  it("sorts by speed and size and searches", () => {
    expect(arrange(list, { ...base, sort: "fastest" })[0].id).toBe("a");
    expect(arrange(list, { ...base, sort: "popular", tab: "community" as const })[0].id).toBeDefined();
    expect(arrange(list, { ...base, sort: "smallest" })[1].id).toBe("b");
    expect(arrange(list, { ...base, query: "B" }).map((x) => x.id)).toEqual(["b"]);
  });
});

describe("goodAt", () => {
  it("shows what a model stands out for, with its real-life looks", () => {
    const g = goodAt(m("z", { tags: ["realism", "fast", "poster"], looks: ["portrait", "food"] }));
    expect(g).toContain("Realistic: portraits and people");
    expect(g).toContain("Fast");
    expect(g).toContain("Poems and posters");
  });
  it("editing jobs are done by the edit models and by models tagged edit", () => {
    const l = [m("g", {}), m("e", { kind: "edit", tags: ["edit", "restore"] }), m("k", { tags: ["edit"] }), m("u", { kind: "upscale" })];
    expect(arrange(l, { ...base, task: "remove" }).map((x) => x.id).sort()).toEqual(["e", "k"]);
    expect(arrange(l, { ...base, task: "restore" }).map((x) => x.id)).toEqual(["e"]);
    expect(arrange(l, { ...base, task: "generate" }).map((x) => x.id).sort()).toEqual(["g", "k"]);
  });
  it("a task filter only narrows when asked", () => {
    const l = [m("a", { kind: "generate" }), m("b", { kind: "generate", tags: ["edit"] })];
    expect(arrange(l, base).length).toBe(2);
    expect(arrange(l, { ...base, task: "edit" }).map((x) => x.id)).toEqual(["b"]);
  });
});
