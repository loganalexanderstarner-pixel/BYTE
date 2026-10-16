// The everyday helpers' rows in Settings (pure data, so it is tested without the screen).
import type { Settings } from "./types";

export const ASK_WHERE: { id: "local" | "cloud" | "auto"; label: string }[] = [
  { id: "local", label: "Local (this computer)" },
  { id: "auto", label: "Auto" },
  { id: "cloud", label: "Cloud" },
];

type Flag = "helperRegion" | "helperExplain" | "helperDialogs" | "helperSmartPaste" | "helperDictate" | "helperPage" | "helperMeeting" | "helperFocus" | "helperTidy" | "helperRecipes" | "helperHa" | "helperGame";
type KeyName = "helperRegionKeys" | "helperExplainKeys" | "helperDictateKeys" | "helperPageKeys";

export interface HelperRow {
  key: Flag;
  title: string;
  text: string;
  keys?: KeyName;
  /** Windows only. */
  windowsOnly?: boolean;
}

const ROWS: HelperRow[] = [
  {
    key: "helperRegion",
    title: "Region screenshot",
    text: "A shortcut lets you drag a box on the screen. BYTE reads only that box (kept in memory, never saved), then offers: what's this, translate, copy the text, make a note, or ask a question.",
    keys: "helperRegionKeys",
    windowsOnly: true,
  },
  {
    key: "helperExplain",
    title: "Explain this",
    text: "Select text in any app and press the shortcut: a small popup next to the pointer explains it. No chat opens, and the app you are reading keeps the keyboard.",
    keys: "helperExplainKeys",
  },
  {
    key: "helperDialogs",
    title: "Error window helper",
    text: "When a new error or installer window appears, BYTE's tray icon shows a red dot: \"BYTE can explain this\". Click it and BYTE reads that window's words (not a picture) and says what to do.",
    windowsOnly: true,
  },
  {
    key: "helperSmartPaste",
    title: "Smart copy toast",
    text: "When you copy a tracking number, an address, a link or a one-time code, a small toast offers the useful action (track it, show it on a map, open or clean the link, clear the code). Never for anything that looks like a password or a key, and nothing is looked up.",
  },
  {
    key: "helperDictate",
    title: "Dictate anywhere",
    text: "Hold the shortcut, speak, let go: the words are typed into the app you are in. The microphone is open only while you hold the key, and the speech is turned into text on this computer. Never typed into a password box, a game or a window on your never-look-at list.",
    keys: "helperDictateKeys",
    windowsOnly: true,
  },
  {
    key: "helperPage",
    title: "Look at this page and help",
    text: "Right-click BYTE's tray icon and choose \"Look at this page and help\": drag a box and BYTE reads only that box and recommends what to do (with guided help on, it can show you or do it for you). Also: select a web address in a browser's address bar and press the shortcut, and BYTE reads that page and tells you more about it. BYTE Cloud sees the page only if you chose Cloud.",
    keys: "helperPageKeys",
    windowsOnly: true,
  },
  {
    key: "helperMeeting",
    title: "Meeting assistant",
    text: "Right-click BYTE's tray icon and choose \"Record a meeting\" (it records only when you click, and shows a popup while it does). BYTE keeps the sound in memory, and when you stop it writes a note with a summary, a to-do list and a transcript (with speaker labels if the speaker models are installed). Tell the others you are recording.",
    windowsOnly: true,
  },
  {
    key: "helperFocus",
    title: "Focus mode",
    text: "Right-click BYTE's tray icon and choose \"Start a focus session\": say what you will work on, and BYTE turns pop-up notifications off, says your plan out loud, times the session, nudges you to take breaks and tells you when time is up. Notifications come back when it ends, when BYTE restarts, or after 12 hours.",
    windowsOnly: true,
  },
  {
    key: "helperTidy",
    title: "Downloads tidy-up",
    text: "When loose files pile up in your Downloads folder, BYTE offers to sort them into folders inside it (Pictures, Documents, Installers, Archives, Music, Videos). Nothing is deleted, files still downloading are left alone, and there is an Undo.",
  },
  {
    key: "helperRecipes",
    title: "Recipes",
    text: "Save a routine as a small file you can share, and add one from someone else after a safety review that shows every step. A recipe can only hold the same few steps routines have, and it never brings a schedule.",
    windowsOnly: true,
  },
  {
    key: "helperHa",
    title: "Home Assistant",
    text: "Say \"turn on the kitchen light\" or \"turn off the desk plug\" and BYTE asks your own Home Assistant on your home network. It only changes lights, plugs, fans and switches, never locks or alarms. The access token is kept in the system credential store.",
  },
  {
    key: "helperGame",
    title: "Game companion",
    text: "In the tray menu: \"Review my last play\" (looks back over the last seconds of the replay) and \"How is my game going?\". Both run on this computer and work without internet. Review needs \"Remember the last seconds\" above.",
    windowsOnly: true,
  },
];

/** Lengths offered for a count of minutes; 0 is "No limit". */
export const MINUTES: number[] = [30, 60, 120, 240, 0];

/** Focus session lengths and break gaps in minutes; 0 is "No limit" / "No reminders". */
export const FOCUS_MINUTES: number[] = [15, 25, 45, 60, 90, 0];
export const BREAK_MINUTES: number[] = [20, 30, 50, 90, 0];

export function breakLabel(n: number): string {
  return n === 0 ? "No reminders" : minutesLabel(n);
}

export function minutesLabel(n: number): string {
  return n === 0 ? "No limit" : n % 60 === 0 ? `${n / 60} hour${n === 60 ? "" : "s"}` : `${n} minutes`;
}

/** The rows to show on this system. */
export function helperRows(windows: boolean): HelperRow[] {
  return ROWS.filter((r) => windows || !r.windowsOnly);
}

export function defaultHelperKeys(windows: boolean): { helperRegionKeys: string; helperExplainKeys: string; helperDictateKeys: string; helperPageKeys: string } {
  return windows ? { helperRegionKeys: "Control+Alt+KeyR", helperExplainKeys: "Control+Alt+KeyE", helperDictateKeys: "Control+Alt+KeyD", helperPageKeys: "Control+Alt+KeyU" } : { helperRegionKeys: "Alt+Super+KeyR", helperExplainKeys: "Alt+Super+KeyE", helperDictateKeys: "Alt+Super+KeyD", helperPageKeys: "Alt+Super+KeyU" };
}

/** The keys in use: the saved ones, else the defaults. */
export function helperKeys(s: Pick<Partial<Settings>, "helperRegionKeys" | "helperExplainKeys" | "helperDictateKeys" | "helperPageKeys">, windows: boolean): { helperRegionKeys: string; helperExplainKeys: string; helperDictateKeys: string; helperPageKeys: string } {
  const d = defaultHelperKeys(windows);
  return { helperRegionKeys: s.helperRegionKeys || d.helperRegionKeys, helperExplainKeys: s.helperExplainKeys || d.helperExplainKeys, helperDictateKeys: s.helperDictateKeys || d.helperDictateKeys, helperPageKeys: s.helperPageKeys || d.helperPageKeys };
}
