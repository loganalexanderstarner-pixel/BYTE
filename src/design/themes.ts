export interface ThemeInfo {
  id: string;
  name: string;
  /** Background, surface, accent, accent-2 — for the picker swatch. */
  swatch: [string, string, string, string];
}

export const THEMES: ThemeInfo[] = [
  { id: "neon-night", name: "Neon Night", swatch: ["#07070f", "#151527", "#00f0ff", "#ff2bd6"] },
  { id: "cyber-pink", name: "Cyber Pink", swatch: ["#12040f", "#260c21", "#ff2bd6", "#ffd02b"] },
  { id: "terminal", name: "Terminal", swatch: ["#030803", "#0b1a0b", "#3cff78", "#b6ff3c"] },
  { id: "midnight", name: "Midnight", swatch: ["#0b1020", "#17203d", "#7aa2ff", "#b48cff"] },
  { id: "ocean", name: "Ocean", swatch: ["#04141c", "#0c2a35", "#2dd4bf", "#38bdf8"] },
  { id: "sunset", name: "Sunset", swatch: ["#170b0a", "#2c1814", "#fb923c", "#f43f5e"] },
  { id: "forest", name: "Forest", swatch: ["#0a120c", "#16261a", "#86efac", "#fde68a"] },
  { id: "light", name: "Light", swatch: ["#f6f7fb", "#ffffff", "#0a84ff", "#bf5af2"] },
  { id: "paper", name: "Paper", swatch: ["#f4efe6", "#fbf8f2", "#9a4d1c", "#2f6f5e"] },
  { id: "high-contrast", name: "High Contrast", swatch: ["#000000", "#111111", "#ffff00", "#00ffff"] },
  { id: "system", name: "Match macOS", swatch: ["#07070f", "#f6f7fb", "#00f0ff", "#0a84ff"] },
];

/** Resolves "system" to a concrete theme based on the macOS appearance. */
export function resolveTheme(id: string): string {
  if (id !== "system") return id;
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "neon-night";
}
