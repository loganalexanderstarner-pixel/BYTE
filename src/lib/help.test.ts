import { describe, expect, it } from "vitest";

import { availableExamples, EXAMPLES, todaysPicks } from "./examples";
import { ARTICLES, linkTarget, searchHelp } from "./help";

const TABS = ["models", "memory", "knowledge", "connectors", "appearance", "engine", "cloud", "about"];

describe("help center", () => {
  it("loads every article with a title and body", () => {
    expect(ARTICLES.length).toBeGreaterThanOrEqual(14);
    for (const a of ARTICLES) {
      expect(a.title.length, a.id).toBeGreaterThan(3);
      expect(a.body.length, a.id).toBeGreaterThan(100);
    }
    expect(ARTICLES[0].id).toBe("getting-started");
  });

  it("links only to real articles and settings tabs", () => {
    const ids = new Set(ARTICLES.map((a) => a.id));
    for (const a of ARTICLES)
      for (const [, href] of a.body.matchAll(/\]\(([^)]+)\)/g)) {
        const t = linkTarget(href);
        if (!t) continue;
        if (t.kind === "help") expect(ids.has(t.id), `${a.id} → ${href}`).toBe(true);
        else expect(TABS, `${a.id} → ${href}`).toContain(t.tab);
      }
  });

  it("searches titles first", () => {
    expect(searchHelp("open anyway")[0].id).toBe("troubleshooting");
    expect(searchHelp("hey byte")[0].id).toBe("voice");
    expect(searchHelp("clipper")[0].id).toBe("notes");
    expect(searchHelp("zzqx")).toEqual([]);
  });
});

describe("example prompts", () => {
  it("only offers what works here", () => {
    const off = availableExamples({ kitchenEnabled: false, macControl: true }, { web: false, mac: false });
    expect(off.some((e) => e.web || e.mac || e.needs === "kitchenEnabled")).toBe(false);
    const all = availableExamples({}, { web: true, mac: true });
    expect(all.length).toBe(EXAMPLES.length);
  });

  it("rotates daily, spread over groups", () => {
    const a = todaysPicks(EXAMPLES, 4, 100);
    expect(a).toEqual(todaysPicks(EXAMPLES, 4, 100));
    expect(new Set(a.map((e) => e.group)).size).toBe(4);
    expect(todaysPicks(EXAMPLES, 4, 101)).not.toEqual(a);
  });
});
