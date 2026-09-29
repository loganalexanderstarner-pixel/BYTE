/** Writing studio helpers: word-level changes, tidying the model's reply, selections. */

export type Action = "rewrite" | "expand" | "shorten" | "tone" | "grammar" | "translate";
/** Languages offered in the studio (any language works in chat: "translate this into …"). */
export const LANGUAGES = ["English", "Spanish", "French", "German", "Italian", "Portuguese", "Dutch", "Polish", "Russian", "Ukrainian", "Turkish", "Arabic", "Hebrew", "Hindi", "Chinese", "Japanese", "Korean", "Vietnamese", "Thai", "Indonesian", "Swedish", "Greek"] as const;
export const TONES = ["friendly", "formal", "confident", "simple", "persuasive"] as const;
export type Tone = (typeof TONES)[number];

/** A run of the result: unchanged, or new (not in the original). */
export interface Piece {
  text: string;
  added: boolean;
}

/** Words and the spaces after them, so the pieces join back into the exact text. */
function tokens(s: string): string[] {
  return s.match(/\S+\s*|\s+/g) ?? [];
}

const norm = (t: string) => t.trim().toLowerCase();

/**
 * The result split into unchanged and new pieces (a longest-common-subsequence over
 * words). Long texts fall back to "all new" rather than doing a slow diff.
 */
export function changes(before: string, after: string): Piece[] {
  const a = tokens(before);
  const b = tokens(after);
  if (a.length * b.length > 4_000_000) return after ? [{ text: after, added: true }] : [];
  // lcs[i][j]: common words in a[i..] and b[j..]
  const lcs: Uint16Array[] = Array.from({ length: a.length + 1 }, () => new Uint16Array(b.length + 1));
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] = norm(a[i]) === norm(b[j]) ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: Piece[] = [];
  const push = (text: string, added: boolean) => {
    const last = out[out.length - 1];
    if (last && last.added === added) last.text += text;
    else out.push({ text, added });
  };
  let i = 0;
  let j = 0;
  while (j < b.length) {
    if (i < a.length && norm(a[i]) === norm(b[j])) {
      push(b[j], false);
      i++;
      j++;
    } else if (i < a.length && lcs[i + 1][j] >= lcs[i][j + 1]) {
      i++; // a word that was removed
    } else {
      push(b[j], !/^\s+$/.test(b[j]));
      j++;
    }
  }
  return out;
}

/** How many words changed (for "12 words changed"). */
export function changedWords(pieces: Piece[]): number {
  return pieces.filter((p) => p.added).reduce((n, p) => n + (p.text.match(/\S+/g)?.length ?? 0), 0);
}

/**
 * The model's reply without what small models add around it: a "Here is the
 * rewritten text:" line, code fences, the <<< >>> markers, or quotes around it all.
 */
export function cleanResult(reply: string): string {
  let t = reply.trim();
  t = t.replace(/^(sure[,!.]?\s*)?(here('s| is| are)|below is)[^\n]*:\s*\n+/i, "");
  t = t.replace(/^```[a-z]*\n([\s\S]*?)\n?```$/i, "$1");
  t = t.replace(/^<<<\s*\n?/, "").replace(/\n?\s*>>>$/, "");
  if (/^["“][\s\S]*["”]$/.test(t) && !/["“”]/.test(t.slice(1, -1))) t = t.slice(1, -1);
  return t.trim();
}

/** The part of the text the action works on: the selection, or all of it. */
export function target(text: string, start: number, end: number): { from: number; to: number } {
  const selected = end > start && text.slice(start, end).trim().length > 0;
  return selected ? { from: start, to: end } : { from: 0, to: text.length };
}

/** `text` with `from..to` replaced by the result (keeping the selection's outer spaces). */
export function applyResult(text: string, from: number, to: number, result: string): string {
  const part = text.slice(from, to);
  const lead = part.match(/^\s*/)?.[0] ?? "";
  const trail = part.match(/\s*$/)?.[0] ?? "";
  return text.slice(0, from) + lead + result.trim() + trail + text.slice(to);
}
