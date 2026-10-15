// Your own themes (Settings → Appearance → Make your own): five colors on top of
// a light or dark base; tokens.css derives the rest, as for the built-in themes.
import { contrast, hex, isHex, isLight, mix } from "./color";

export interface ThemeColors {
  bg: string;
  panel: string;
  border: string;
  text: string;
  accent: string;
}

export interface CustomTheme {
  /** "custom:<slug>" */
  id: string;
  name: string;
  colors: ThemeColors;
}

export const KEYS: (keyof ThemeColors)[] = ["bg", "panel", "border", "text", "accent"];
export const LABELS: Record<keyof ThemeColors, string> = { bg: "Background", panel: "Panels", border: "Lines", text: "Text", accent: "Accent" };
export const MAX_CUSTOM = 10;

export const isCustom = (id: string | undefined | null): boolean => !!id?.startsWith("custom:");

export function slug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 32) || "theme";
}

/** Plain-language problems with readability (same checks as the built-in themes' test). */
export function themeWarnings(c: ThemeColors): string[] {
  if (!KEYS.every((k) => isHex(c[k]))) return ["Every color must be a hex value like #1a2b3c."];
  const [bg, panel, text, accent] = [c.bg, c.panel, c.text, c.accent].map(hex);
  const out: string[] = [];
  if (contrast(text, bg) < 4.5 || contrast(text, panel) < 4.5) out.push("Text is hard to read on the background or panels.");
  if (contrast(mix(text, bg, 0.52), panel) < 3) out.push("Faint labels will be hard to see.");
  if (contrast(accent, panel) < 3) out.push("The accent color doesn't stand out from the panels.");
  const onAccent = isLight(c.bg) ? hex("#ffffff") : bg;
  if (contrast(onAccent, accent) < 4.5) out.push("Labels on accent buttons will be hard to read.");
  return out;
}

/** A theme from settings or an imported file, or null when it isn't one. */
export function parseTheme(v: unknown): CustomTheme | null {
  if (!v || typeof v !== "object") return null;
  const o = v as Record<string, unknown>;
  const name = typeof o.name === "string" ? o.name.trim().slice(0, 40) : "";
  const colors = o.colors as Record<string, unknown> | undefined;
  if (!name || !colors || !KEYS.every((k) => typeof colors[k] === "string" && isHex(colors[k] as string))) return null;
  const c = Object.fromEntries(KEYS.map((k) => [k, (colors[k] as string).trim().toLowerCase()])) as unknown as ThemeColors;
  return { id: `custom:${slug(name)}`, name, colors: c };
}

/** The `.bytetheme` file: small JSON anyone can share. */
export function exportTheme(t: CustomTheme): string {
  return JSON.stringify({ byteTheme: 1, name: t.name, colors: t.colors }, null, 2);
}

export function importTheme(text: string): CustomTheme {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    throw new Error("That file isn't a BYTE theme.");
  }
  const t = parseTheme(v);
  if (!t) throw new Error("That theme is missing a name or has colors BYTE can't use.");
  return t;
}

/** Adds or replaces (by id) a theme, keeping at most MAX_CUSTOM. */
export function upsert(list: CustomTheme[], t: CustomTheme): CustomTheme[] {
  const rest = list.filter((x) => x.id !== t.id);
  if (rest.length >= MAX_CUSTOM) throw new Error(`You can keep up to ${MAX_CUSTOM} of your own themes. Delete one first.`);
  return [...rest, t];
}

/** Puts a custom theme on the page: the matching base for status colors, and its five colors. */
export function applyCustom(root: HTMLElement, t: CustomTheme | null): void {
  for (const k of KEYS) root.style.removeProperty(`--${k}`);
  if (!t) return;
  root.dataset.theme = isLight(t.colors.bg) ? "light" : "midnight";
  for (const k of KEYS) root.style.setProperty(`--${k}`, t.colors[k]);
}
