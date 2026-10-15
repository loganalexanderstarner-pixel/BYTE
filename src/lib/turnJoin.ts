/**
 * "Wait until I'm done": a spoken turn that clearly isn't finished ("and then", "um", ending in and / but / the / to) gets about one
 * more second before it is sent. wake.rs keeps listening for that second (`wake://more`); this decides what to do with what comes back.
 */

const FILLERS = new Set(["um", "uh", "er", "erm", "hmm", "mm", "uhh", "umm", "ah", "eh"]);
/** Words a finished sentence does not end on. */
const DANGLING = new Set([
  "and", "but", "or", "so", "the", "a", "an", "to", "of", "for", "with", "at", "then", "because", "if", "when",
  "are", "my", "your", "his", "her", "their", "from", "by", "about", "into", "also", "plus", "than", "as",
]);

const words = (text: string): string[] =>
  text
    .toLowerCase()
    .replace(/[^a-z' ]/g, " ")
    .split(/\s+/)
    .filter(Boolean);

/** True when the words stop mid-thought: a filler or a joining word last, or nothing but a filler. */
export function endsUnfinished(text: string): boolean {
  const w = words(text);
  if (w.length === 0) return false;
  const last = w[w.length - 1];
  if (FILLERS.has(last)) return true;
  // A sentence that ended with a full stop, question mark or exclamation mark is finished, even on a short word ("what is it?").
  if (/[.?!]["')\]]?\s*$/.test(text.trim())) return false;
  return DANGLING.has(last);
}

/** The words of both parts as one turn: trailing fillers and a dangling comma of the first part are dropped. */
export function joinTurns(first: string, more: string): string {
  const a = first.trim().split(/\s+/).filter(Boolean);
  while (a.length && FILLERS.has((a[a.length - 1] ?? "").toLowerCase().replace(/[^a-z']/g, ""))) a.pop();
  const head = a.join(" ").replace(/[,;:\s]+$/, "");
  const tail = more.trim();
  if (!tail) return head || first.trim();
  if (!head) return tail;
  return `${head} ${tail}`;
}

export type JoinAction = { kind: "send"; text: string } | { kind: "wait" } | { kind: "transcribe"; wav: string };

/**
 * One turn's joining. Events can come in either order (the text of the first part, and the audio heard in the extra second), so both
 * are remembered until the other is here. The extra second is only ever used once per turn.
 */
export class TurnJoiner {
  private held: string | null = null;
  /** undefined: not arrived yet; null: nothing was said; string: audio (base64 WAV). */
  private more: string | null | undefined = undefined;
  private asked = false;

  reset(): void {
    this.held = null;
    this.more = undefined;
    this.asked = false;
  }

  /** The text of the turn is ready. */
  firstText(text: string, enabled: boolean): JoinAction {
    if (!enabled || !endsUnfinished(text)) return { kind: "send", text };
    this.held = text;
    return this.decide();
  }

  /** `wake://more` came: audio heard after the first part, or null. */
  moreArrived(wav: string | null): JoinAction {
    this.more = wav;
    return this.held === null ? { kind: "wait" } : this.decide();
  }

  /** The extra audio was turned into words. */
  moreText(text: string): JoinAction {
    const held = this.held ?? "";
    this.reset();
    return { kind: "send", text: joinTurns(held, text) };
  }

  /** Gave up waiting: send what is held (or nothing). */
  giveUp(): JoinAction {
    const held = this.held;
    this.reset();
    return held === null ? { kind: "wait" } : { kind: "send", text: held };
  }

  /** The first part is held, waiting for the extra second. */
  holding(): boolean {
    return this.held !== null;
  }

  private decide(): JoinAction {
    if (this.more === undefined) return { kind: "wait" };
    if (this.more === null || this.asked) return this.giveUp();
    this.asked = true;
    return { kind: "transcribe", wav: this.more };
  }
}
