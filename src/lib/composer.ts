import { api } from "./api";
import { cleanResult } from "./writing";

/** Tones for "Rephrase", as the writing studio names them (Rust `writing::instructions`). */
export const TONES = [
  { id: "formal", label: "Formal" },
  { id: "friendly", label: "Friendly" },
  { id: "fun", label: "Fun" },
  { id: "sympathetic", label: "Sympathetic" },
] as const;

/** The three versions "Ideas" offers. */
export const IDEA_TONES = ["friendly", "fun", "formal"] as const;

/** One writing-studio action on a short text, returned whole (it streams, but a text is short). */
export async function rewrite(text: string, action: "grammar" | "rewrite" | "shorten" | "tone" | "reply", tone: string | null = null): Promise<string> {
  let out = "";
  await api.writingRun(crypto.randomUUID(), text, action, tone, false, (e) => {
    if (e.kind === "content") out += e.delta;
  });
  return tidy(cleanResult(out));
}

/** A text message keeps no quotes around it and no "Here's a…" lead-in line. */
export function tidy(s: string): string {
  let t = s.trim();
  const lines = t.split("\n");
  if (lines.length > 1 && /^(here('|’)s|sure|okay|ok)\b.*:\s*$/i.test(lines[0].trim())) t = lines.slice(1).join("\n").trim();
  if (/^["“].*["”]$/s.test(t)) t = t.slice(1, -1).trim();
  return t;
}
