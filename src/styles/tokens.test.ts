import { describe, expect, it } from "vitest";

import { THEMES } from "../design/themes";
import css from "./tokens.css?raw";

type RGB = [number, number, number];
const hex = (h: string): RGB => {
  const s = h.replace("#", "");
  const full = s.length === 3 ? [...s].map((c) => c + c).join("") : s;
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16)) as RGB;
};
/** color-mix(in srgb, a p, b) */
const mix = (a: RGB, b: RGB, p: number): RGB => a.map((v, i) => v * p + b[i] * (1 - p)) as RGB;
const lum = ([r, g, b]: RGB) => {
  const ch = (v: number) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b);
};
const contrast = (a: RGB, b: RGB) => {
  const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m);
  return (x + 0.05) / (y + 0.05);
};

/** The hex tokens a theme block sets. */
function block(id: string): Record<string, string> {
  // The theme's own block is the last one naming it (the shared light-theme group comes first).
  const m = [...css.matchAll(new RegExp(`\\[data-theme="${id}"\\] \\{([^}]*)\\}`, "g"))].pop();
  if (!m) return {};
  return Object.fromEntries([...m[1].matchAll(/--([a-z0-9-]+):\s*(#[0-9a-fA-F]{3,6});/g)].map((x) => [x[1], x[2]]));
}

/** A theme's colors after the derivations in :root (same ratios as tokens.css). */
function resolved(id: string) {
  const t = block(id);
  const [bg, panel, text, accent] = [t.bg, t.panel, t.text, t.accent].map(hex);
  return {
    bg,
    panel,
    text,
    accent,
    surface2: t["surface-2"] ? hex(t["surface-2"]) : mix(panel, text, 0.94),
    text2: t["text-2"] ? hex(t["text-2"]) : mix(text, bg, 0.72),
    text3: t["text-3"] ? hex(t["text-3"]) : mix(text, bg, 0.52),
    onAccent: t["accent-contrast"] ? hex(t["accent-contrast"]) : THEMES.find((x) => x.id === id)?.light ? hex("#ffffff") : bg,
  };
}

const themes = THEMES.filter((t) => t.id !== "system");

describe("themes", () => {
  it("has 20 themes, each with a block that sets the five colors", () => {
    expect(themes).toHaveLength(20);
    for (const t of themes) {
      const b = block(t.id);
      for (const k of ["bg", "panel", "border", "text", "accent"]) expect(b[k], `${t.id} --${k}`).toMatch(/^#/);
    }
    const inCss = [...css.matchAll(/\/\* -+ .* -+ \*\/\n\[data-theme="([a-z-]+)"\] \{/g)].map((m) => m[1]);
    expect(new Set(inCss)).toEqual(new Set(themes.map((t) => t.id)));
  });

  it("defines every token in :root, so no theme can leave one undefined", () => {
    const root = css.slice(css.indexOf(":root {\n  color-scheme"), css.indexOf("/* Light themes"));
    for (const k of ["surface", "surface-2", "surface-3", "border-strong", "text-2", "text-3", "accent-2", "accent-contrast", "accent-soft", "glow", "glow-strong", "ok", "warn", "danger", "user-bubble", "code-bg", "shadow", "scrim", "bg-grad"])
      expect(root, k).toContain(`--${k}:`);
  });

  it.each(themes.map((t) => t.id))("%s is readable", (id) => {
    const c = resolved(id);
    // Body text (4.5 is WCAG AA for normal text).
    expect(contrast(c.text, c.bg), "text on bg").toBeGreaterThanOrEqual(4.5);
    expect(contrast(c.text, c.panel), "text on panel").toBeGreaterThanOrEqual(4.5);
    expect(contrast(c.text, c.surface2), "text on surface-2").toBeGreaterThanOrEqual(4.5);
    expect(contrast(c.text2, c.panel), "secondary text").toBeGreaterThanOrEqual(4.5);
    // Faint text (labels, hints) and the accent as a UI color: 3.0.
    expect(contrast(c.text3, c.panel), "faint text").toBeGreaterThanOrEqual(3);
    expect(contrast(c.accent, c.panel), "accent on panel").toBeGreaterThanOrEqual(3);
    // Labels on accent-filled buttons.
    expect(contrast(c.onAccent, c.accent), "text on accent").toBeGreaterThanOrEqual(4.5);
  });
});
