/** Global shortcuts in Tauri's syntax ("Alt+Super+KeyB"), shown the Mac way ("⌥⌘B"). Rust: `quick::parse_keys`, `quick::pretty`. */

const MODS: [string, string][] = [
  ["Control", "⌃"],
  ["Alt", "⌥"],
  ["Shift", "⇧"],
  ["Super", "⌘"],
];

/** "Alt+Super+KeyB" → "⌥⌘B". */
export function prettyKeys(keys: string): string {
  let mods = "";
  let key = "";
  for (const part of keys.split("+").map((p) => p.trim())) {
    const l = part.toLowerCase();
    if (l === "ctrl" || l === "control") mods += "⌃";
    else if (l === "alt" || l === "option") mods += "⌥";
    else if (l === "shift") mods += "⇧";
    else if (["super", "cmd", "command", "meta", "cmdorctrl", "commandorcontrol"].includes(l)) mods += "⌘";
    else key = part.replace(/^Key/, "").replace(/^Digit/, "");
  }
  // The modifiers in the Mac's own order (⌃⌥⇧⌘).
  const ordered = MODS.map(([, s]) => s).filter((s) => mods.includes(s)).join("");
  return ordered + (key.toLowerCase() === "space" ? "Space" : key);
}

/**
 * A key press → shortcut text, or null while only modifiers are down. Uses `code`
 * (the physical key), so ⌥ doesn't turn B into "∫".
 */
export function keysFromEvent(e: Pick<KeyboardEvent, "code" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey">): string | null {
  if (!e.code || /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/.test(e.code)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Control");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Super");
  parts.push(e.code);
  return parts.join("+");
}

/** Why these keys can't be a global shortcut (null when they can). */
export function keysProblem(keys: string): string | null {
  const parts = keys.split("+");
  if (!parts.some((p) => ["Control", "Alt", "Super"].includes(p))) return "Add ⌘, ⌥ or ⌃, so normal typing still works.";
  if (parts.length < 2) return "Press a key with the modifiers.";
  return null;
}
