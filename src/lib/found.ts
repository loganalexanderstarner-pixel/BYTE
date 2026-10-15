// Foundations: typed calls for safe mode, crash recovery, what's new, battery mode, the daily summary and data removal.
// Kept apart from api.ts so the shared files stay small.
import { invoke } from "@tauri-apps/api/core";

export interface SafeStatus {
  active: boolean;
  /** Settings that were on and are held off for this run. */
  heldOff: string[];
}

export const foundApi = {
  safeStatus: () => invoke<SafeStatus>("safe_mode_status"),
  safeRestart: (safe: boolean) => invoke<void>("safe_mode_restart", { safe }),
};

/** The words for what safe mode held off, for the banner. */
export function safeBannerText(s: SafeStatus): string {
  const n = s.heldOff.length;
  const base = "Safe mode: Hey BYTE, PC control, screen features, the browser and other optional features are off for this run. Your saved settings were not changed.";
  return n === 0 ? base : `${base} ${n} ${n === 1 ? "feature was" : "features were"} on and is held off.`;
}

export interface CrashFound {
  version: string;
  panic: string | null;
}

export const crashApi = {
  status: () => invoke<{ found: CrashFound | null }>("crash_status"),
  dismiss: () => invoke<void>("crash_dismiss"),
  build: () => invoke<{ text: string; includes: string[] }>("crash_report_build"),
  save: (path: string, text: string) => invoke<void>("problem_report_save", { path, text }),
};

/** The line on the card: which version stopped, in plain words. */
export function crashCardText(c: CrashFound): string {
  const v = c.version ? ` (version ${c.version})` : "";
  return `BYTE closed unexpectedly last time${v}. Nothing was lost that was already saved.`;
}
