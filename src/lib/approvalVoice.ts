// Spoken approvals: BYTE says what it wants to do and listens for "yes" or "no" through the same listener as "Hey BYTE".
// Nothing here ever approves by itself: no clear answer means no answer, and the card's own timer still ends in "no".
import { invoke } from "@tauri-apps/api/core";

export type Verdict = "yes" | "no" | "unclear";

const YES = new Set(["yes", "yeah", "yep", "yup", "sure", "ok", "okay", "go ahead", "do it", "approve", "approved", "please do", "yes please", "go for it", "that's fine", "confirm"]);
const NO = new Set(["no", "nope", "nah", "don't", "do not", "dont", "stop", "cancel", "deny", "never mind", "nevermind", "don't do it", "do not do it", "not now"]);

/** What a short spoken answer means. Anything long or mixed is unclear: a clear yes or no is the only thing that counts. */
export function parseYesNo(text: string): Verdict {
  const t = text
    .toLowerCase()
    .replace(/[.,!?;:]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!t) return "unclear";
  const bare = t.replace(/^(hey byte |byte )/, "").replace(/ (please|byte)$/, "").trim();
  if (YES.has(bare) || YES.has(t)) return "yes";
  if (NO.has(bare) || NO.has(t)) return "no";
  const words = bare.split(" ");
  if (words.length <= 3) {
    const y = words.some((w) => YES.has(w));
    const n = words.some((w) => NO.has(w));
    if (y && !n) return "yes";
    if (n && !y) return "no";
  }
  return "unclear";
}

export interface VoiceAsk {
  id: string;
  question: string;
  waitMs: number;
}

/** The approval waiting for a spoken answer in this window, if any. */
let pending: { id: string; tries: number } | null = null;

export const approvalVoice = {
  pending: () => pending,
  begin(id: string) {
    pending = { id, tries: 0 };
  },
  clear() {
    pending = null;
  },
  /** Applies a heard answer: returns what to do next. */
  hear(text: string): { say: string | null; answered: boolean; listenAgain: boolean } {
    const p = pending;
    if (!p) return { say: null, answered: false, listenAgain: false };
    const v = parseYesNo(text);
    if (v !== "unclear") {
      pending = null;
      void invoke("agent_approve", { id: p.id, ok: v === "yes", edits: null });
      return { say: v === "yes" ? "Okay, doing it." : "Okay, I won't.", answered: true, listenAgain: false };
    }
    p.tries += 1;
    if (p.tries >= 2) {
      pending = null;
      return { say: "I didn't catch that. You can use the buttons on screen.", answered: false, listenAgain: false };
    }
    return { say: "Please say yes or no.", answered: false, listenAgain: true };
  },
};
