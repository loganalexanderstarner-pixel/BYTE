// Help articles are written for a Mac and worded for the machine they are shown on.
//
// Where the steps differ, an article has platform blocks, each marker on a line of its own:
//
//   <!-- mac -->   what only a Mac reader sees
//   <!-- win -->   what only a Windows reader sees
//   <!-- linux --> what only a Linux reader sees
//   <!-- pc -->    what a Windows or a Linux reader sees (anything that is not a Mac)
//   <!-- all -->   back to text for everyone (also the default at the top of a file)
//
// Everything else is reworded mechanically on Windows and Linux (this Mac → this PC, ⌘ → Ctrl+,
// the menu bar → the system tray). The same rules are in src-tauri/src/platform_text.rs for the
// copy of these articles BYTE shows its model; keep them in step.
import { platformKeys } from "./keys";
import { osText, type Os } from "./platform";

/** A boolean is the old form: true for a Windows reader, false for a Mac reader. */
function asOs(os: Os | boolean): Os {
  return typeof os === "boolean" ? (os ? "windows" : "mac") : os;
}

/** The article with only the blocks for this platform, and no markers. */
export function forPlatform(text: string, os: Os | boolean): string {
  const here = asOs(os);
  let mode: "all" | "mac" | "win" | "linux" | "pc" = "all";
  const out: string[] = [];
  for (const line of text.split("\n")) {
    const marker = /^\s*<!--\s*(mac|win|linux|pc|all)\s*-->\s*$/.exec(line);
    if (marker) {
      mode = marker[1] as typeof mode;
      continue;
    }
    const shown = mode === "all" || (mode === "pc" && here !== "mac") || mode === (here === "windows" ? "win" : here);
    if (shown) out.push(line);
  }
  return out.join("\n");
}

/** An article as the reader on this machine should see it. The identity on a Mac apart from the blocks. */
export function localize(text: string, os: Os | boolean): string {
  const here = asOs(os);
  const picked = forPlatform(text, here);
  return here === "mac" ? picked : osText(platformKeys(picked, true), here);
}
