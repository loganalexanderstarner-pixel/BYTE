// The everyday helpers' rows in Settings (pure data, so it is tested without the screen).
import type { Settings } from "./types";

export const ASK_WHERE: { id: "local" | "cloud" | "auto"; label: string }[] = [
  { id: "local", label: "Local (this computer)" },
  { id: "auto", label: "Auto" },
  { id: "cloud", label: "Cloud" },
];

type Flag = "helperRegion" | "helperExplain" | "helperDialogs" | "helperSmartPaste" | "helperDictate" | "helperPage" | "helperMeeting" | "helperFocus" | "helperTidy" | "helperRecipes" | "helperHa" | "helperGame" | "helperNav" | "helperRead" | "helperCaptions" | "helperPagetools" | "helperEmail" | "helperProduct" | "helperForm" | "helperRewrite" | "helperExplorer" | "helperScreenmem";
type KeyName = "helperRegionKeys" | "helperExplainKeys" | "helperDictateKeys" | "helperPageKeys" | "helperReadKeys" | "helperRewriteKeys";

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
  {
    key: "helperNav",
    title: "Voice navigation",
    text: "An accessibility mode: say \"go back\", \"scroll to the bottom\", \"next tab\", \"click Send\" and BYTE does it in the program in front, without the mouse. It uses PC control, so PC control must be on too; every step is logged, Stop works, and risky buttons still ask first.",
    windowsOnly: true,
  },
  {
    key: "helperRead",
    title: "Read aloud with highlighting",
    text: "Select text in any app and press the shortcut: BYTE reads it in a small window that highlights the sentence being spoken, at the speed you pick. Press the shortcut again to stop. Uses one of BYTE's downloaded voices.",
    keys: "helperReadKeys",
  },
  {
    key: "helperCaptions",
    title: "Live captions",
    text: "Right-click BYTE's tray icon and choose \"Start live captions\": BYTE listens to the sound this computer plays, writes captions in a small window, and \"What did they just say?\" turns the last 30 seconds into words. The sound stays in memory only. Needs a speech model.",
    windowsOnly: true,
  },
  {
    key: "helperPagetools",
    title: "Page tools",
    text: "Right-click BYTE's tray icon and choose \"Page tools\" with a page open in your browser: make a to-do from it, save it as a note, extract its tables to spreadsheet files, or compare it with another tab. If you select text first, only that text is used. The page text is read on this computer (or BYTE Cloud only if you chose Cloud).",
    windowsOnly: true,
  },
  {
    key: "helperEmail",
    title: "Email helper",
    text: "Select an email thread in your browser or mail program, choose \"Page tools\" > \"Help with this email\", and BYTE gives a short summary and three reply drafts (friendly, formal, short). You pick one and it is copied for you to paste. BYTE never sends anything.",
    windowsOnly: true,
  },
  {
    key: "helperProduct",
    title: "Product helper",
    text: "On a product page, choose \"Page tools\" > \"Research this product\": BYTE searches the web for its price and reviews, reads a few pages and summarises them with the sources. The product name is sent to a search engine.",
    windowsOnly: true,
  },
  {
    key: "helperForm",
    title: "Form helper",
    text: "Save your name, email, phone and address here once. On a form, choose \"Page tools\" > \"Fill this form\": BYTE shows what it will type and types it only after you agree (PC control must be on). It never fills passwords, card numbers or security codes, and never presses Submit.",
    windowsOnly: true,
  },
  {
    key: "helperRewrite",
    title: "Fix my writing in place",
    text: "Select text in any app and press the shortcut: fix mistakes, or make it friendlier, more formal, more confident or shorter, then \"Replace the selection\" puts the result back (you see it first). The same popup can explain selected code, show the steps for selected maths, or translate foreign text.",
    keys: "helperRewriteKeys",
  },
  {
    key: "helperExplorer",
    title: "Explorer right-click: Ask BYTE",
    text: "Adds \"Ask BYTE\" to the right-click menu of files for you only; choosing it opens the file attached in a new BYTE chat (nothing is sent until you send it). Turning this off removes the entry. Windows cannot take files dropped on a tray icon, so drop files on the BYTE window instead.",
    windowsOnly: true,
  },
  {
    key: "helperScreenmem",
    title: "Searchable memory of my screen",
    text: "The most careful helper, so it is last and off until you turn it on. Every few minutes BYTE reads the words of the window in front and keeps only those words (never a picture) on this computer, so you can search \"what was I reading about X?\". Windows on your never-look-at list, password managers, banks, private windows, games and times you are away are skipped; card numbers and keys are blanked. It never leaves this computer and uses no model. You can clear everything at any time.",
    windowsOnly: true,
  },
];

/** How often it looks, in minutes, and how long it keeps, in days (0 = until you clear it). */
export const SCREENMEM_EVERY: number[] = [1, 3, 5, 10, 30];
export const SCREENMEM_DAYS: number[] = [1, 7, 14, 30, 90, 0];

export function keepLabel(days: number): string {
  return days === 0 ? "Until I clear it" : days === 1 ? "1 day" : `${days} days`;
}

export const READ_SPEEDS: { id: "slow" | "normal" | "fast"; label: string }[] = [
  { id: "slow", label: "Slow" },
  { id: "normal", label: "Normal" },
  { id: "fast", label: "Fast" },
];

const COMMON = ["hey", "okay", "hello", "the", "and", "you", "that", "this", "what", "yes", "stop", "play", "start", "computer play", "hey you"];

/** Why a wake phrase can't be used (the same rules as the app's), or null. Empty means "Hey BYTE". */
export function wakePhraseProblem(phrase: string): string | null {
  if (!phrase.trim()) return null;
  if (/[^\p{L} '-]/u.test(phrase)) return "Use only letters and spaces.";
  const words = phrase.toLowerCase().split(/\s+/).map((w) => w.replace(/[^\p{L}\p{N}]/gu, "")).filter(Boolean);
  const letters = words.join("").length;
  if (words.length === 0 || words.length > 3) return "Use one to three words.";
  if (letters < 6) return "That is too short (6 letters at least), so it would trigger by accident.";
  if (letters > 30) return "That is too long to say.";
  if (COMMON.includes(words.join(" "))) return "That is too common a word: it would trigger all the time.";
  return null;
}

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

export function defaultHelperKeys(windows: boolean): { helperRegionKeys: string; helperExplainKeys: string; helperDictateKeys: string; helperPageKeys: string; helperReadKeys: string; helperRewriteKeys: string } {
  return windows ? { helperRegionKeys: "Control+Alt+KeyR", helperExplainKeys: "Control+Alt+KeyE", helperDictateKeys: "Control+Alt+KeyD", helperPageKeys: "Control+Alt+KeyU", helperReadKeys: "Control+Alt+KeyL", helperRewriteKeys: "Control+Alt+KeyW" } : { helperRegionKeys: "Alt+Super+KeyR", helperExplainKeys: "Alt+Super+KeyE", helperDictateKeys: "Alt+Super+KeyD", helperPageKeys: "Alt+Super+KeyU", helperReadKeys: "Alt+Super+KeyL", helperRewriteKeys: "Alt+Super+KeyW" };
}

/** The keys in use: the saved ones, else the defaults. */
export function helperKeys(s: Pick<Partial<Settings>, "helperRegionKeys" | "helperExplainKeys" | "helperDictateKeys" | "helperPageKeys" | "helperReadKeys" | "helperRewriteKeys">, windows: boolean): { helperRegionKeys: string; helperExplainKeys: string; helperDictateKeys: string; helperPageKeys: string; helperReadKeys: string; helperRewriteKeys: string } {
  const d = defaultHelperKeys(windows);
  return { helperRegionKeys: s.helperRegionKeys || d.helperRegionKeys, helperExplainKeys: s.helperExplainKeys || d.helperExplainKeys, helperDictateKeys: s.helperDictateKeys || d.helperDictateKeys, helperPageKeys: s.helperPageKeys || d.helperPageKeys, helperReadKeys: s.helperReadKeys || d.helperReadKeys, helperRewriteKeys: s.helperRewriteKeys || d.helperRewriteKeys };
}
