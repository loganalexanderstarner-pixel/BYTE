// The everyday helpers' rows in Settings (pure data, so it is tested without the screen).
import type { Settings } from "./types";

export const ASK_WHERE: { id: "local" | "cloud" | "auto"; label: string }[] = [
  { id: "local", label: "Local (this computer)" },
  { id: "auto", label: "Auto" },
  { id: "cloud", label: "Cloud" },
];

type Flag = "helperRegion" | "helperExplain" | "helperDialogs" | "helperSmartPaste";
type KeyName = "helperRegionKeys" | "helperExplainKeys";

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
];

/** The rows to show on this system. */
export function helperRows(windows: boolean): HelperRow[] {
  return ROWS.filter((r) => windows || !r.windowsOnly);
}

export function defaultHelperKeys(windows: boolean): { helperRegionKeys: string; helperExplainKeys: string } {
  return windows ? { helperRegionKeys: "Control+Alt+KeyR", helperExplainKeys: "Control+Alt+KeyE" } : { helperRegionKeys: "Alt+Super+KeyR", helperExplainKeys: "Alt+Super+KeyE" };
}

/** The keys in use: the saved ones, else the defaults. */
export function helperKeys(s: Pick<Partial<Settings>, "helperRegionKeys" | "helperExplainKeys">, windows: boolean): { helperRegionKeys: string; helperExplainKeys: string } {
  const d = defaultHelperKeys(windows);
  return { helperRegionKeys: s.helperRegionKeys || d.helperRegionKeys, helperExplainKeys: s.helperExplainKeys || d.helperExplainKeys };
}
