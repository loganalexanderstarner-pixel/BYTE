import { describe, expect, it } from "vitest";

import { fuzzyScore, rankItems, type PaletteItem } from "./palette";

const items: PaletteItem[] = [
  { id: "new", label: "New chat", group: "Actions", hint: "⌘N" },
  { id: "settings", label: "Settings: Appearance", keywords: "theme colors preferences", group: "Settings" },
  { id: "deep", label: "Mode: Deep", group: "Modes" },
  { id: "c1", label: "Trip to Lisbon in May", group: "Chats" },
  { id: "c2", label: "Sourdough starter help", group: "Chats" },
];

describe("palette search", () => {
  it("needs every letter in order", () => {
    expect(fuzzyScore("lsbn", "Trip to Lisbon")).toBeGreaterThan(0);
    expect(fuzzyScore("nbsl", "Trip to Lisbon")).toBe(0);
  });

  it("puts substrings and word starts first", () => {
    expect(fuzzyScore("lis", "Trip to Lisbon")).toBeGreaterThan(fuzzyScore("lsb", "Trip to Lisbon"));
    expect(rankItems(items, "new")[0].id).toBe("new");
    expect(rankItems(items, "deep")[0].id).toBe("deep");
    expect(rankItems(items, "sour")[0].id).toBe("c2");
  });

  it("finds by keywords and keeps order when empty", () => {
    expect(rankItems(items, "theme")[0].id).toBe("settings");
    expect(rankItems(items, "").map((i) => i.id)).toEqual(items.map((i) => i.id));
    expect(rankItems(items, "zzz")).toEqual([]);
  });
});
