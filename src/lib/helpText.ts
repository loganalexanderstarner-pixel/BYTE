// Help articles are written for a Mac and worded for the machine they are shown on.
//
// Where the steps differ, an article has platform blocks, each marker on a line of its own:
//
//   <!-- mac -->   what only a Mac reader sees
//   <!-- win -->   what only a Windows reader sees
//   <!-- all -->   back to text for everyone (also the default at the top of a file)
//
// Everything else is reworded mechanically on Windows (this Mac → this PC, ⌘ → Ctrl+, the
// menu bar → the system tray). The same rules are in src-tauri/src/platform_text.rs for the
// copy of these articles BYTE shows its model; keep them in step.
import { platformKeys } from "./keys";
import { osText } from "./platform";

/** The article with only the blocks for this platform, and no markers. */
export function forPlatform(text: string, windows: boolean): string {
  let mode: "all" | "mac" | "win" = "all";
  const out: string[] = [];
  for (const line of text.split("\n")) {
    const marker = /^\s*<!--\s*(mac|win|all)\s*-->\s*$/.exec(line);
    if (marker) {
      mode = marker[1] as typeof mode;
      continue;
    }
    if (mode === "all" || (mode === "win") === windows) out.push(line);
  }
  return out.join("\n");
}

/** An article as the reader on this machine should see it. The identity on a Mac apart from the blocks. */
export function localize(text: string, windows: boolean): string {
  const picked = forPlatform(text, windows);
  return windows ? osText(platformKeys(picked, true), true) : picked;
}
