// @vitest-environment jsdom
import { describe, expect, it } from "vitest";

import { applyCustom, exportTheme, importTheme, isCustom, MAX_CUSTOM, parseTheme, themeWarnings, upsert, type CustomTheme } from "./customTheme";

const good = { bg: "#0f1420", panel: "#171d2c", border: "#2a3247", text: "#e7ebf3", accent: "#4c8dff" };

describe("custom themes", () => {
  it("checks readability like the built-in themes", () => {
    expect(themeWarnings(good)).toEqual([]);
    expect(themeWarnings({ ...good, text: "#2a3247" })[0]).toMatch(/Text is hard to read/);
    expect(themeWarnings({ ...good, accent: "#1b2233" }).join(" ")).toMatch(/accent color/);
    expect(themeWarnings({ ...good, bg: "blue" })).toEqual(["Every color must be a hex value like #1a2b3c."]);
  });

  it("round-trips a .bytetheme file and refuses junk", () => {
    const t = parseTheme({ name: "Night Owl", colors: good })!;
    expect(t.id).toBe("custom:night-owl");
    expect(isCustom(t.id)).toBe(true);
    expect(importTheme(exportTheme(t))).toEqual(t);
    expect(() => importTheme("not json")).toThrow(/isn't a BYTE theme/);
    expect(() => importTheme(JSON.stringify({ name: "X", colors: { ...good, accent: "red; background: url(x)" } }))).toThrow(/can't use/);
    expect(parseTheme({ colors: good })).toBeNull();
  });

  it("keeps at most ten, replacing by name", () => {
    let list: CustomTheme[] = [];
    for (let i = 0; i < MAX_CUSTOM; i++) list = upsert(list, parseTheme({ name: `T${i}`, colors: good })!);
    expect(list).toHaveLength(MAX_CUSTOM);
    expect(upsert(list, parseTheme({ name: "T3", colors: { ...good, accent: "#ff0000" } })!)).toHaveLength(MAX_CUSTOM);
    expect(() => upsert(list, parseTheme({ name: "One more", colors: good })!)).toThrow(/up to 10/);
  });

  it("applies the colors on a light or dark base", () => {
    const root = document.createElement("html");
    applyCustom(root, parseTheme({ name: "Day", colors: { ...good, bg: "#fafafa", panel: "#ffffff", text: "#111111", accent: "#1d4ed8" } }));
    expect(root.dataset.theme).toBe("light");
    expect(root.style.getPropertyValue("--accent")).toBe("#1d4ed8");
    applyCustom(root, null);
    expect(root.style.getPropertyValue("--accent")).toBe("");
  });
});
