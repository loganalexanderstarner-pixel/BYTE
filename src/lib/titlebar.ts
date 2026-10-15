import { getCurrentWindow } from "@tauri-apps/api/window";

import { inTauri } from "./api";
import { isWindows } from "./keys";

/** How light a colour looks, 0 (black) to 1 (white): the usual weighting of red, green and blue. */
export function lightness([r, g, b]: [number, number, number]): number {
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

/** "rgb(15, 20, 32)" or "rgba(15, 20, 32, 1)" → [15, 20, 32]; null when it is not that form. */
export function parseRgb(css: string): [number, number, number] | null {
  const m = css.match(/\d+(\.\d+)?/g);
  if (!m || m.length < 3) return null;
  return [Number(m[0]), Number(m[1]), Number(m[2])];
}

/** Whether a CSS colour in any form (hex, rgb(), a name) is dark, read through the browser. */
export function isDarkColor(css: string): boolean {
  const probe = document.createElement("span");
  probe.style.color = css;
  document.body.appendChild(probe);
  const rgb = parseRgb(getComputedStyle(probe).color);
  probe.remove();
  return rgb ? lightness(rgb) < 0.5 : true;
}

/**
 * Makes Windows' own title bar follow the BYTE theme. On a Mac the title bar is an overlay
 * and takes the app's colours by itself; on Windows it is the system's, light by default,
 * which is a bright strip across a dark app. Decided from the theme's real background, so
 * custom themes work too. Does nothing off Windows, so the Mac is untouched.
 */
export function syncTitlebar(root: HTMLElement): void {
  if (!inTauri || !isWindows()) return;
  const bg = getComputedStyle(root).getPropertyValue("--bg").trim() || "#0f1420";
  void getCurrentWindow()
    .setTheme(isDarkColor(bg) ? "dark" : "light")
    .catch(() => undefined);
}
