// Foundations: typed calls for safe mode, crash recovery, what's new, battery mode, the daily summary and data removal.
// Kept apart from api.ts so the shared files stay small.
import { invoke } from "@tauri-apps/api/core";

import type { ModelStatus } from "./types";

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

export interface WhatsNew {
  version: string;
  notes: string;
  show: boolean;
}

export const whatsNewApi = {
  status: () => invoke<WhatsNew>("whatsnew_status"),
  seen: () => invoke<void>("whatsnew_seen"),
};

/** The release notes cut for the card: the title without its phase and version, and the "New in this version" part
 *  (or everything after the first paragraph when a file has no such heading). */
export function whatsNewParts(notes: string): { title: string; body: string } {
  const lines = notes.replace(/\r\n/g, "\n").split("\n");
  const first = lines.findIndex((l) => l.startsWith("# "));
  const raw = first >= 0 ? lines[first].slice(2).trim() : "";
  const title = raw.replace(/\s*\(v?[\d.]+[^)]*\)\s*$/, "").replace(/^Phase \d+( improvements)?\s*[—-]\s*/, "").trim() || "What's new";
  const rest = lines.slice(first + 1);
  const start = rest.findIndex((l) => /^##\s+new\b/i.test(l));
  let body: string[];
  if (start >= 0) {
    const end = rest.findIndex((l, i) => i > start && /^##\s/.test(l));
    body = rest.slice(start + 1, end < 0 ? undefined : end);
  } else {
    body = rest.filter((l) => !/^##\s+(not in this version|what byte can do)/i.test(l));
  }
  return { title, body: body.join("\n").trim() };
}

export interface RemovePlan {
  paths: [string, number][];
  totalBytes: number;
}

export const removeApi = {
  plan: (chats: boolean, models: boolean) => invoke<RemovePlan>("data_remove_plan", { chats, models }),
  remove: (chats: boolean, models: boolean, confirm: string) => invoke<void>("data_remove", { chats, models, confirm }),
};

/** The file name at the end of a path, for the list the person confirms. */
export function leafName(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() ?? p;
}

export interface PowerStatus {
  onBattery: boolean;
  laptopMode: boolean;
  paused: string[];
}

export const powerApi = { status: () => invoke<PowerStatus>("power_status") };

/** A downloaded chat model clearly smaller than the one in use (at most 70% of its size), the largest such, or null. */
export function smallerModel(models: ModelStatus[], activeKey: string | null | undefined): { key: string; name: string } | null {
  if (!activeKey) return null;
  let activeSize = 0;
  for (const m of models) for (const v of m.variants) if (v.key === activeKey) activeSize = v.sizeBytes;
  if (!activeSize) return null;
  let best: { key: string; name: string; size: number } | null = null;
  for (const m of models) {
    if (m.role !== "chat") continue;
    for (const v of m.variants) {
      if (!v.installed || v.key === activeKey || v.sizeBytes > activeSize * 0.7) continue;
      if (!best || v.sizeBytes > best.size) best = { key: v.key, name: m.name, size: v.sizeBytes };
    }
  }
  return best && { key: best.key, name: best.name };
}

/** The badge's words. */
export function batteryBadgeText(p: PowerStatus): string {
  if (!p.laptopMode || !p.onBattery) return "";
  return p.paused.length ? `On battery: ${p.paused.join(" and ")} paused` : "On battery";
}

/** The choices for "keep researching until I'm back". Unlimited is one of them. */
export const UNTIL_BACK_CHOICES: { value: NonNullable<import("./types").Settings["browserResearchUntilBack"]>; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "15m", label: "15 minutes" },
  { value: "1h", label: "1 hour" },
  { value: "3h", label: "3 hours" },
  { value: "8h", label: "8 hours" },
  { value: "unlimited", label: "Unlimited" },
];

export const OWN_EDGE_WARNING =
  "Use your own Edge profile?\n\nBYTE would open Edge with YOUR everyday profile: the sites you are signed in to, your saved logins and your history. Pages BYTE opens can use those sign-ins, and what BYTE reads there is visible to BYTE.\n\n" +
  "BYTE still asks before it submits, buys, sends, deletes or downloads anything, never types passwords or card numbers, and blocks banking sites. Close Edge first. Newer Edge versions may refuse to be controlled this way; BYTE then uses its own separate profile instead.\n\nTurn this on only if you understand this.";
