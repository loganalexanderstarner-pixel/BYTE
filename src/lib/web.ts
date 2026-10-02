// The Web pill's three states: Off, Auto (search when needed) and Always
// (search for every real question). Stored as `webSearch` (on/off, as
// before) plus `webMode` ("auto" | "always").

import type { Settings } from "./types";

export type WebState = "off" | "auto" | "always";

export function webState(s: Pick<Settings, "webSearch" | "webMode"> | null | undefined): WebState {
  if (!s?.webSearch) return "off";
  return s.webMode === "always" ? "always" : "auto";
}

/** The next state when the pill is clicked: Off → Auto → Always → Off. */
export function nextWeb(state: WebState): Pick<Settings, "webSearch" | "webMode"> {
  switch (state) {
    case "off":
      return { webSearch: true, webMode: "auto" };
    case "auto":
      return { webSearch: true, webMode: "always" };
    case "always":
      return { webSearch: false, webMode: "auto" };
  }
}

export const WEB_LABEL: Record<WebState, string> = { off: "Web off", auto: "Web: Auto", always: "Web: Always" };

export const WEB_HINT: Record<WebState, string> = {
  off: "Web is off: BYTE answers from what it already knows. Click for Auto.",
  auto: "Web: Auto. BYTE searches when a question needs current or specific information. Click for Always.",
  always: "Web: Always. BYTE searches and reads sources for every real question. Click to turn the web off.",
};
