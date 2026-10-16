// The live research card's state and the words that end a job ("I'm back"), shared by the card, the chat box and the voice flow.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface LiveResearch {
  active: boolean;
  pages: number;
  elapsedSecs: number;
  /** null: no time limit. */
  limitSecs: number | null;
  standby: boolean;
  tabs: boolean;
}

/** "I'm back", "im back now", "hey byte, I am back", "stop researching": the person ending a research job in words. */
export function isImBack(text: string): boolean {
  const t = text
    .toLowerCase()
    .replace(/[’`]/g, "'")
    .replace(/[.,!?;:]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^(hey |ok |okay )?byte /, "")
    .replace(/ (now|byte|please|everyone|guys)$/, "")
    .trim();
  return ["i'm back", "im back", "i am back", "i'm here", "im here", "i'm home", "i'm back now", "stop researching", "that's enough research", "that is enough", "done researching", "i'm back thanks", "ok i'm back", "i'm back byte"].includes(t) || /^(ok |okay |alright |hi |hello )?(i'm|im|i am) back( now)?$/.test(t);
}

let live: LiveResearch | null = null;
const watchers = new Set<(l: LiveResearch | null) => void>();

function set(l: LiveResearch | null) {
  live = l && l.active ? l : null;
  watchers.forEach((w) => w(live));
}

export const researchLive = {
  current: () => live,
  watch(cb: (l: LiveResearch | null) => void): () => void {
    watchers.add(cb);
    cb(live);
    return () => void watchers.delete(cb);
  },
  /** Tells the running job the person is back. */
  back: () => invoke<boolean>("research_back"),
  /** The chat box and the voice flow call this first: while a job runs, "I'm back" ends it instead of starting a new message. */
  handle(text: string): boolean {
    if (!live || !isImBack(text)) return false;
    void researchLive.back();
    return true;
  },
};

if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
  void listen<LiveResearch>("research://live", (e) => set(e.payload)).catch(() => undefined);
}

/** "12 minutes" / "1 h 5 min" for the card. */
export function elapsedText(secs: number): string {
  const m = Math.floor(secs / 60);
  if (m < 1) return "under a minute";
  if (m < 60) return `${m} minute${m === 1 ? "" : "s"}`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** One line for the card. */
export function liveLine(l: LiveResearch): string {
  const where = l.tabs ? "in Edge tabs you can watch" : "in the background";
  const left = l.limitSecs == null ? "no time limit" : `up to ${elapsedText(l.limitSecs)}`;
  if (l.standby) return `BYTE has read everything it planned (${l.pages} pages) and is standing by ${where}. ${elapsedText(l.elapsedSecs)} so far, ${left}.`;
  return `BYTE is researching ${where}: ${l.pages} page${l.pages === 1 ? "" : "s"} so far, ${elapsedText(l.elapsedSecs)}, ${left}.`;
}
