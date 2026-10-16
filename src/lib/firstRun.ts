// First-run setup: the optional features that are off until the person says yes, and the settings each "yes" changes.
// Pure data and helpers (no React): the screen is components/onboarding/FirstRunFeatures.tsx.
import type { CapableModel } from "./control";
import type { Settings } from "./types";

export type FirstRunId = "heyByte" | "pcControl" | "screenAsk" | "browser" | "guidedHelp";

export interface FirstRunFeature {
  id: FirstRunId;
  title: string;
  /** One plain sentence: what it does and what it reads. */
  explain: string;
  /** Needs the Windows app. */
  windowsOnly?: boolean;
}

export const FIRST_RUN_FEATURES: FirstRunFeature[] = [
  { id: "heyByte", title: "Hey BYTE", explain: "BYTE listens through the microphone for the words \"Hey BYTE\" so you can talk to it hands-free; the sound is checked on this computer and not kept." },
  { id: "pcControl", title: "PC control", explain: "BYTE can open, close and click things on this PC when you ask by voice or text, and it asks you first for anything risky.", windowsOnly: true },
  { id: "screenAsk", title: "Screen questions", explain: "When you ask about what is on your screen, BYTE looks at that one window, in memory only, and never saves the picture.", windowsOnly: true },
  { id: "browser", title: "Browser", explain: "BYTE can open web pages itself in a window you can watch, with a Stop button, to look things up for you." },
  { id: "guidedHelp", title: "Guided help", explain: "When you ask how to do something in a program, BYTE can draw a ring around each button in turn, or do it for you.", windowsOnly: true },
];

/** The features this system can offer. A phone gets none of them here. */
export function featuresFor(windows: boolean, phone: boolean): FirstRunFeature[] {
  if (phone) return [];
  return FIRST_RUN_FEATURES.filter((f) => windows || !f.windowsOnly);
}

/** The recommended PC-control model from the control status (the first on its list), if it is not the chat model's own. */
export function controlOffer(models: CapableModel[] | undefined): CapableModel | null {
  const m = models?.find((x) => x.downloadKey || x.installedKey);
  return m ?? null;
}

/** What the offer costs to download, in bytes (0 when it is already on disk). */
export function offerBytes(m: CapableModel | null): number {
  return m && !m.installedKey ? m.downloadBytes : 0;
}

/**
 * The settings to save for what the person ticked. Nothing ticked changes nothing: no key is written for a feature
 * that stays off, so a saved setting is never overwritten by a skipped question.
 */
export function featurePatch(ticked: FirstRunId[], controlModelKey: string | null): Partial<Settings> {
  const on = new Set(ticked);
  const patch: Partial<Settings> = {};
  if (on.has("heyByte")) patch.wakeWord = true;
  if (on.has("pcControl")) {
    patch.pcControl = true;
    if (controlModelKey) patch.controlModel = controlModelKey;
  }
  if (on.has("screenAsk")) patch.screenAsk = true;
  if (on.has("browser")) {
    patch.browserVisible = true;
    patch.browserLiveResearch = true;
  }
  if (on.has("guidedHelp")) patch.guidedHelp = true;
  return patch;
}
