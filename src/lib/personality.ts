// How BYTE talks: five 1–5 sliders (prompt.rs turns non-default ones into instructions) and presets.
export interface Personality {
  warmth: number;
  length: number;
  humor: number;
  formality: number;
  opinions: number;
}

export const BALANCED: Personality = { warmth: 3, length: 3, humor: 3, formality: 3, opinions: 3 };

export const PRESETS: { id: string; name: string; hint: string; p: Personality }[] = [
  { id: "balanced", name: "Balanced", hint: "BYTE's usual voice", p: BALANCED },
  { id: "friendly", name: "Friendly", hint: "Warm, casual, a little playful", p: { warmth: 5, length: 3, humor: 4, formality: 2, opinions: 3 } },
  { id: "concise", name: "Concise", hint: "Short, straight to the point", p: { warmth: 2, length: 1, humor: 2, formality: 3, opinions: 4 } },
  { id: "professional", name: "Professional", hint: "Polished and neutral", p: { warmth: 2, length: 3, humor: 1, formality: 5, opinions: 2 } },
  { id: "teacher", name: "Teacher", hint: "Patient, thorough, with examples", p: { warmth: 4, length: 5, humor: 3, formality: 3, opinions: 3 } },
  { id: "coach", name: "Coach", hint: "Encouraging and decisive", p: { warmth: 5, length: 2, humor: 3, formality: 2, opinions: 5 } },
];

export const SLIDERS: { key: keyof Personality; label: string; low: string; high: string }[] = [
  { key: "warmth", label: "Warmth", low: "Matter-of-fact", high: "Warm" },
  { key: "length", label: "Length", low: "Brief", high: "Thorough" },
  { key: "humor", label: "Humor", low: "None", high: "Playful" },
  { key: "formality", label: "Formality", low: "Casual", high: "Formal" },
  { key: "opinions", label: "Opinions", low: "Neutral", high: "Opinionated" },
];

/** The preset these sliders match, if any. */
export function presetOf(p: Personality | null | undefined): string | null {
  const v = p ?? BALANCED;
  return PRESETS.find((x) => SLIDERS.every(({ key }) => x.p[key] === v[key]))?.id ?? null;
}

/** A slider's position in words ("A bit warm", "Very brief"). */
export function sliderWord(value: number, low: string, high: string): string {
  if (value <= 1) return `Very ${low.toLowerCase()}`;
  if (value === 2) return `A bit ${low.toLowerCase()}`;
  if (value === 4) return `A bit ${high.toLowerCase()}`;
  if (value >= 5) return `Very ${high.toLowerCase()}`;
  return "Normal";
}
