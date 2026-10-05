/** Global shortcuts in Tauri's syntax ("Alt+Super+KeyB"), shown the Mac way ("⌥⌘B") or the Windows way ("Alt+Win+B"). Rust: `quick::parse_keys`, `quick::pretty`. */

/** The selected-text hotkey before the backend has said otherwise: ⌥⌘B on a Mac, Ctrl+Alt+B on Windows
 *  (Alt+Win+B is already Windows' HDR toggle). Matches `selection::HOTKEY` in Rust. */
export function defaultSelectionKeys(windows: boolean = isWindows()): string {
  return windows ? "Control+Alt+KeyB" : "Alt+Super+KeyB";
}

/** Quick Ask's shortcut before the backend has said otherwise. Not Alt+Space on Windows: that is the
 *  window menu of every app there. Matches `settings::default_quick_keys` in Rust. */
export function defaultQuickAskKeys(windows: boolean = isWindows()): string {
  return windows ? "Control+Alt+Space" : "Alt+Space";
}

/** True in the Windows app (WebView2 reports "Win32"); false on a Mac and in tests. */
export function isWindows(): boolean {
  return typeof navigator !== "undefined" && /^Win/i.test(navigator.platform ?? "");
}

/**
 * Rewrites Mac key glyphs inside UI text for Windows: "New chat (⌘N)" becomes
 * "New chat (Ctrl+N)". The identity on a Mac. The in-app handlers already accept
 * Ctrl as well as Command, so only what is SHOWN needed changing.
 */
export function platformKeys(text: string, windows: boolean = isWindows()): string {
  if (!windows) return text;
  return text.replace(/⌘/g, "Ctrl+").replace(/⌃/g, "Ctrl+").replace(/⌥/g, "Alt+").replace(/⇧/g, "Shift+");
}

/**
 * "Alt+Super+KeyB" → "⌥⌘B" on a Mac, "Alt+Win+B" on Windows. The platform is a
 * parameter so both forms can be checked anywhere; it defaults to the real one.
 * On Windows "Super" is the Windows key and "CmdOrCtrl" is Ctrl.
 */
export function prettyKeys(keys: string, windows: boolean = isWindows()): string {
  let ctrl = false;
  let alt = false;
  let shift = false;
  let meta = false;
  let key = "";
  for (const part of keys.split("+").map((p) => p.trim())) {
    const l = part.toLowerCase();
    if (l === "ctrl" || l === "control") ctrl = true;
    else if (l === "alt" || l === "option") alt = true;
    else if (l === "shift") shift = true;
    else if (["cmdorctrl", "commandorcontrol"].includes(l)) {
      if (windows) ctrl = true;
      else meta = true;
    } else if (["super", "cmd", "command", "meta"].includes(l)) meta = true;
    else key = part.replace(/^Key/, "").replace(/^Digit/, "");
  }
  const shown = key.toLowerCase() === "space" ? "Space" : key;
  if (windows) {
    // Windows names its modifiers, in the order Windows itself lists them.
    return [ctrl && "Ctrl", alt && "Alt", shift && "Shift", meta && "Win", shown].filter(Boolean).join("+");
  }
  // The modifiers in the Mac's own order (⌃⌥⇧⌘).
  return (ctrl ? "⌃" : "") + (alt ? "⌥" : "") + (shift ? "⇧" : "") + (meta ? "⌘" : "") + shown;
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
export function keysProblem(keys: string, windows: boolean = isWindows()): string | null {
  const parts = keys.split("+");
  if (!parts.some((p) => ["Control", "Alt", "Super"].includes(p))) {
    return windows ? "Add Ctrl, Alt or Win, so normal typing still works." : "Add ⌘, ⌥ or ⌃, so normal typing still works.";
  }
  if (parts.length < 2) return "Press a key with the modifiers.";
  return null;
}
