import { api, inTauri } from "./api";

export interface NoticeContext {
  /** The `spokenNotices` setting. */
  enabled: boolean;
  /** Speech is on: reading answers aloud or Talk mode, on a system that can speak. */
  speechOn: boolean;
  /** A game is running (`null` when it could not be read). */
  game: boolean | null;
  /** Another program is on a call (`null` when it could not be read). */
  meeting: boolean | null;
}

/**
 * Should BYTE say this notice out loud? Only when the person turned spoken notices on and speech is on, and never during a
 * game or a call. When it could not be told whether a game or a call is going, stay quiet: a missed notice is better than
 * a voice over a meeting. Timers do not come through here: they always ring.
 */
export function shouldSpeakNotice(c: NoticeContext): boolean {
  if (!c.enabled || !c.speechOn) return false;
  return c.game === false && c.meeting === false;
}

/** "Your Qwen3 14B model finished downloading." */
export function downloadedLine(name: string): string {
  return name ? `Your ${name} model finished downloading.` : "Your model finished downloading.";
}

/** "Your X finished." for long jobs. */
export function jobDoneLine(what: string): string {
  return `${what.charAt(0).toUpperCase()}${what.slice(1)} finished.`;
}

/** Looks up the live game and call state, then speaks `text` through `speak` if allowed. Never throws. */
export async function announce(text: string, base: Omit<NoticeContext, "game" | "meeting">, speak: (text: string) => Promise<void> | void, meetingQuiet = true): Promise<boolean> {
  if (!base.enabled || !base.speechOn || !inTauri) return false;
  const [game, meeting] = await Promise.all([
    api.pcBusy().then((b) => b.busy, () => null),
    meetingQuiet ? api.meetingNow().then((m) => m, () => null) : Promise.resolve(false),
  ]);
  if (!shouldSpeakNotice({ ...base, game, meeting })) return false;
  await speak(text);
  return true;
}
