import { isWindows } from "../lib/keys";
import { osText } from "../lib/platform";

export interface ThemeInfo {
  id: string;
  name: string;
  /** Background, panel, accent, accent-2: for the picker swatch. */
  swatch: [string, string, string, string];
  light?: boolean;
}

/** Every theme in styles/tokens.css (tokens.test.ts keeps the two in step). Midnight is the default. */
export const THEMES: ThemeInfo[] = [
  { id: "midnight", name: "Midnight", swatch: ["#0f1420", "#171d2c", "#4c8dff", "#4c8dff"] },
  { id: "neon-night", name: "Neon Night", swatch: ["#07070f", "#0e0e1a", "#00f0ff", "#ff2bd6"] },
  { id: "steelers", name: "Steelers", swatch: ["#0a0a0a", "#161616", "#ffb612", "#ffb612"] },
  { id: "terminal", name: "Terminal", swatch: ["#000000", "#0a0f0a", "#33ff33", "#33ff33"] },
  { id: "ocean", name: "Ocean", swatch: ["#071a1f", "#0d2830", "#2dd4bf", "#38bdf8"] },
  { id: "cyber-pink", name: "Cyber Pink", swatch: ["#12040f", "#1c0818", "#ff2bd6", "#ffd02b"] },
  { id: "aurora", name: "Aurora", swatch: ["#0b0f1a", "#121829", "#5eead4", "#c084fc"] },
  { id: "graphite", name: "Graphite", swatch: ["#111214", "#1a1b1e", "#8ab4f8", "#8ab4f8"] },
  { id: "fjord", name: "Fjord", swatch: ["#1d222b", "#252b36", "#88c0d0", "#b48ead"] },
  { id: "lavender", name: "Lavender", swatch: ["#151223", "#1d1930", "#b69cff", "#ff9ecf"] },
  { id: "rose", name: "Rose", swatch: ["#1a0d12", "#24131a", "#ff7aa2", "#ffc2d1"] },
  { id: "ember", name: "Ember", swatch: ["#140a06", "#1e110b", "#ff6b35", "#ffc145"] },
  { id: "sunset", name: "Sunset", swatch: ["#170b0a", "#221210", "#fb923c", "#f43f5e"] },
  { id: "mocha", name: "Mocha", swatch: ["#1c1512", "#251c18", "#d4a373", "#e9c46a"] },
  { id: "forest", name: "Forest", swatch: ["#0a120c", "#111c14", "#86efac", "#fde68a"] },
  { id: "light", name: "Light", swatch: ["#f4f6fb", "#ffffff", "#3b6fd9", "#3b6fd9"], light: true },
  { id: "paper", name: "Paper", swatch: ["#faf6ee", "#ffffff", "#96592a", "#96592a"], light: true },
  { id: "sand", name: "Sand", swatch: ["#f7f1e3", "#fffaf0", "#8a5a00", "#2f6f5e"], light: true },
  { id: "mint", name: "Mint", swatch: ["#eef8f3", "#ffffff", "#0f7a52", "#2563eb"], light: true },
  { id: "high-contrast", name: "High Contrast", swatch: ["#000000", "#000000", "#ffff00", "#00ffff"] },
  { id: "system", name: isWindows() ? "Match Windows" : osText("Match macOS"), swatch: ["#0f1420", "#f4f6fb", "#4c8dff", "#3b6fd9"] },
];

export const DEFAULT_THEME = "midnight";

/** Resolves "system" to a concrete theme based on the system appearance (macOS or Windows). */
export function resolveTheme(id: string): string {
  if (id !== "system") return id;
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : DEFAULT_THEME;
}
