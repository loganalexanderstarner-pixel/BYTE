// PC control (Windows): what the Rust side `control/` sends to Settings. Kept apart from api.ts so the shared files stay small.
import { invoke } from "@tauri-apps/api/core";

export interface CapableModel {
  id: string;
  name: string;
  note: string;
  installedKey: string | null;
  downloadKey: string | null;
  downloadBytes: number;
  selected: boolean;
}

export interface ControlStatus {
  supported: boolean;
  active: boolean;
  modelLabel: string;
  models: CapableModel[];
}

export interface LogEntry {
  at: string;
  what: string;
  ok: boolean;
  detail: string;
}

export type RoutineStep =
  | { type: "openApp"; name: string }
  | { type: "closeApp"; name: string }
  | { type: "volume"; percent: number }
  | { type: "doNotDisturb"; on: boolean }
  | { type: "openUrl"; url: string }
  | { type: "runAutomation"; name: string };

export interface Routine {
  id: string;
  phrase: string;
  steps: RoutineStep[];
  schedule: string;
  enabled: boolean;
}

export const controlApi = {
  status: () => invoke<ControlStatus>("control_status"),
  log: (limit = 100) => invoke<LogEntry[]>("control_log", { limit }),
  logClear: () => invoke<void>("control_log_clear"),
  stop: () => invoke<void>("control_stop"),
  runningApps: () => invoke<string[]>("control_running_apps"),
  checkFolder: (path: string) => invoke<string>("control_find_folder_check", { path }),
  routines: () => invoke<Routine[]>("routines_list"),
  routineSave: (routine: Routine) => invoke<Routine>("routine_save", { routine }),
  routineDelete: (id: string) => invoke<void>("routine_delete", { id }),
  routineRun: (id: string) => invoke<string>("routine_run", { id }),
  scheduleParse: (text: string) => invoke<[string, string] | null>("routine_schedule_parse", { text }),
  guideStart: (id: string) => invoke<void>("guide_start", { id }),
  guideDo: (id: string) => invoke<string>("guide_do", { id }),
  guideDismiss: (id: string) => invoke<void>("guide_dismiss", { id }),
  guideCommand: (what: "next" | "skip" | "stop" | "replan") => invoke<void>("guide_command", { what }),
  download: (key: string) => invoke<void>("model_download", { key }),
};

export const STEP_KINDS: { type: RoutineStep["type"]; label: string }[] = [
  { type: "openApp", label: "Open a program" },
  { type: "closeApp", label: "Close a program" },
  { type: "volume", label: "Set the volume" },
  { type: "doNotDisturb", label: "Notifications on or off" },
  { type: "openUrl", label: "Open a web address" },
  { type: "runAutomation", label: "Run an automation" },
];

/** A new, empty step of a kind. */
export function newStep(type: RoutineStep["type"]): RoutineStep {
  switch (type) {
    case "openApp":
    case "closeApp":
    case "runAutomation":
      return { type, name: "" };
    case "volume":
      return { type, percent: 40 };
    case "doNotDisturb":
      return { type, on: true };
    case "openUrl":
      return { type, url: "https://" };
  }
}

/** One line for a step, as the menu lists it. */
export function stepLabel(s: RoutineStep): string {
  switch (s.type) {
    case "openApp":
      return `Open ${s.name || "…"}`;
    case "closeApp":
      return `Close ${s.name || "…"}`;
    case "volume":
      return `Volume to ${s.percent}%`;
    case "doNotDisturb":
      return s.on ? "Turn notifications off" : "Turn notifications on";
    case "openUrl":
      return `Open ${s.url || "…"}`;
    case "runAutomation":
      return `Run the automation "${s.name || "…"}"`;
  }
}

/** What is missing before a routine can be saved, or null. The Rust side checks again. */
export function routineProblem(r: Routine): string | null {
  const phrase = r.phrase.trim();
  if (phrase.length < 3) return "Say what the phrase is, like “game time”.";
  if (r.steps.length === 0) return "Add at least one step.";
  for (const [i, s] of r.steps.entries()) {
    if ((s.type === "openApp" || s.type === "closeApp" || s.type === "runAutomation") && !s.name.trim()) return `Step ${i + 1} needs a name.`;
    if (s.type === "openUrl" && !/^https?:\/\/\S+\.\S+/i.test(s.url.trim())) return `Step ${i + 1} needs a web address that starts with http:// or https://.`;
    if (s.type === "volume" && (s.percent < 0 || s.percent > 100)) return `Step ${i + 1}: the volume is 0 to 100.`;
  }
  return null;
}

export function sizeText(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  return gb >= 10 ? `${Math.round(gb)} GB` : `${gb.toFixed(1)} GB`;
}

/** Allowed programs as stored: lower case, no .exe, no blanks, no repeats. */
export function addApp(list: string[], name: string): string[] {
  const n = name.trim().toLowerCase().replace(/\.exe$/, "");
  if (!n || n.length > 60 || !/^[\p{L}\p{N} ._-]+$/u.test(n) || list.includes(n)) return list;
  return [...list, n];
}

export interface ControlState {
  active: boolean;
  goal: string;
}

/** The banner's sentence while BYTE is operating the PC. */
export function bannerText(goal: string): string {
  const g = goal.trim().replace(/\s+/g, " ");
  const short = g.length > 80 ? `${g.slice(0, 79)}…` : g;
  return short ? `BYTE is controlling your PC: ${short}` : "BYTE is controlling your PC";
}

export type GuideState = "offer" | "showing" | "doing" | "done" | "dismissed";

/** What the card says under its steps for a state. */
export function guideStatus(state: GuideState, summary: string): string {
  switch (state) {
    case "showing":
      return "Follow the ring. F8 for next, F9 to look again, Esc to stop (or say next, skip, stop).";
    case "doing":
      return "BYTE is doing it. Press Stop in the bar at the bottom to end it.";
    case "done":
      return summary || "Finished.";
    default:
      return "";
  }
}
