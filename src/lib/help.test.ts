import { describe, expect, it } from "vitest";

import { availableExamples, EXAMPLES, todaysPicks } from "./examples";
import { ARTICLES, linkTarget, searchHelp, sectioned } from "./help";

const TABS = ["general", "models", "voice", "control", "privacy", "memory", "cloud", "backup", "images", "knowledge", "connectors", "appearance", "engine", "about"];

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

  it("opens the new settings groups from help links", () => {
    expect(linkTarget("byte-setting:general")).toEqual({ kind: "setting", tab: "general" });
    expect(linkTarget("byte-setting:backup")).toEqual({ kind: "setting", tab: "backup" });
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

describe("help sections", () => {
  it("puts every article in exactly one section", () => {
    const placed = sectioned(ARTICLES).flatMap((s) => s.articles.map((a) => a.id));
    expect([...placed].sort()).toEqual(ARTICLES.map((a) => a.id).sort());
  });

  it("starts with Getting started and has no empty section", () => {
    const s = sectioned(ARTICLES);
    expect(s[0].title).toBe("Start here");
    expect(s[0].articles[0].id).toBe("getting-started");
    expect(s.every((x) => x.articles.length > 0)).toBe(true);
  });
});
