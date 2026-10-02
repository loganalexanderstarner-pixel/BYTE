/** Hands-free conversation: when to stop listening, and when the user wants to stop talking to BYTE. */

export type Hearing = "waiting" | "speech" | "done" | "nothing";

/**
 * Decides from the mic's level (0–1, ~every 85 ms) when a spoken turn is over: after `silenceMs` of quiet
 * following speech, or "nothing" after `waitMs` without any speech. The quiet level adapts to the room.
 */
export class SilenceDetector {
  private floor = 0.02;
  private heard = 0;
  private lastLoud = 0;
  private readonly started: number;

  constructor(
    now: number,
    private readonly silenceMs = 1200,
    private readonly waitMs = 20_000,
    private readonly minSpeechMs = 250,
  ) {
    this.started = now;
  }

  push(level: number, now: number): Hearing {
    const loud = level > Math.max(this.floor * 2.5, 0.06);
    if (loud) {
      if (!this.heard) this.heard = now;
      this.lastLoud = now;
    } else if (!this.heard) {
      this.floor = this.floor * 0.9 + level * 0.1;
    }
    const spoke = this.heard && this.lastLoud - this.heard >= this.minSpeechMs;
    if (spoke && now - this.lastLoud >= this.silenceMs) return "done";
    if (!spoke && now - this.started >= this.waitMs) return "nothing";
    return spoke || this.heard ? "speech" : "waiting";
  }
}

/** "Stop", "that's all", "goodbye"… ends hands-free mode instead of being sent. */
export function isStopPhrase(text: string): boolean {
  const t = text
    .toLowerCase()
    .replace(/[^a-z' ]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  return /^(ok(ay)? )?(stop( listening| talking)?|that'?s all|that is all|goodbye|bye( bye)?( byte)?|thanks? (byte|you),? (that'?s all|bye)|never ?mind|end conversation|we'?re done)( byte| thanks| thank you)?$/.test(t);
}

/** Ends a "Hey BYTE" follow-up: a thank-you or a stop phrase. */
export function isDone(text: string): boolean {
  const t = text
    .toLowerCase()
    .replace(/[^a-z' ]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  return isStopPhrase(text) || /^(no,? )?(thanks|thank you|cheers|that'?s it|perfect thanks|great thanks|ok(ay)? thanks?)( byte| a lot| so much)?$/.test(t);
}
