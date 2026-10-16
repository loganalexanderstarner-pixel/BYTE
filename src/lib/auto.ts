// Autonomous mode (Windows): what the Rust side `control/auto/` sends to Settings and the chat card, plus the small pure helpers.
import { invoke } from "@tauri-apps/api/core";

export const REMEMBER_FIELD = "__remember";

export type AutonomyLevel = "ask" | "contract" | "never";

export const LEVELS: { value: AutonomyLevel; label: string; text: string }[] = [
  { value: "ask", label: "Ask for risky steps", text: "BYTE shows a card before anything risky: closing a window, sending, deleting, opening files. This is how it works today." },
  { value: "contract", label: "Ask only about things outside a run contract", text: "Steps inside the programs, folders and people you named go ahead. Anything outside them waits for your yes." },
  { value: "never", label: "Never ask, just do it", text: "No approval cards at all. Program files, dangerous key combinations and payments are still refused, and Stop always works." },
];

export interface Rule {
  id: string;
  kind: string;
  detail: string;
  app: string;
  user: string;
  created: string;
  text: string;
}

export interface Contract {
  apps: string[];
  folders: string[];
  mayMessage: string[];
  neverDelete: boolean;
  neverSpend: boolean;
  timeBudget: string;
  stepBudget: string;
  messageBudget: string;
}

export interface StageDraft {
  text: string;
  tool: string;
}

export interface Sent {
  to: string;
  what: string;
}

export type StageStatus = "pending" | "running" | "done" | "skipped" | "failed";
export type JobState = "planned" | "running" | "paused" | "done" | "stopped" | "failed";

export interface Stage extends StageDraft {
  status: StageStatus;
  summary: string;
  reason: string;
  steps: number;
  ms: number;
  read: string[];
  sent: Sent[];
  files: string[];
}

export interface Job {
  id: string;
  title: string;
  request: string;
  stages: Stage[];
  contract: Contract;
  state: JobState;
  current: number;
  created: string;
  usedMs: number;
  steps: number;
  messages: number;
  stopReason: string;
  chatId: string;
}

export interface FeedItem {
  app: string;
  what: string;
  ok: boolean | null;
  detail: string;
}

export interface CheckInView {
  id: string;
  title: string;
  fields: [string, string][];
  canRemember: boolean;
}

export interface LiveView {
  jobId: string;
  title: string;
  stagesTotal: number;
  stagesDone: number;
  stage: number;
  stageText: string;
  next: string;
  now: string;
  status: "idle" | "running" | "waiting" | "paused" | "done" | "stopped" | "failed";
  note: string;
  percent: number;
  feed: FeedItem[];
  checkIn: CheckInView | null;
}

export interface AutoStatus {
  supported: boolean;
  running: boolean;
  view: LiveView;
  tools: string[];
}

export interface AuditEntry {
  at: string;
  job: string;
  stage: string;
  app: string;
  what: string;
  ok: boolean;
  result: string;
}

export interface Recording {
  steps: import("./control").RoutineStep[];
  typedParts: number;
  secretSkipped: number;
  cut: number;
}

export const autoApi = {
  rules: () => invoke<Rule[]>("auto_rules_list"),
  ruleRemove: (id: string) => invoke<void>("auto_rule_remove", { id }),
  rulesClear: () => invoke<void>("auto_rules_clear"),
  status: () => invoke<AutoStatus>("auto_status"),
  jobs: () => invoke<Job[]>("auto_jobs"),
  jobDelete: (id: string) => invoke<void>("auto_job_delete", { id }),
  report: (id: string) => invoke<string>("auto_report", { id }),
  split: (request: string) => invoke<StageDraft[]>("auto_split", { request }),
  start: (request: string, stages: StageDraft[], contract: Contract) => invoke<string>("auto_start", { request, stages, contract }),
  resume: (id: string, contract?: Contract) => invoke<string>("auto_job_resume", { id, contract }),
  stop: () => invoke<void>("auto_stop"),
  pause: () => invoke<void>("auto_pause"),
  unpause: () => invoke<void>("auto_resume"),
  answer: (id: string, answer: "yes" | "always" | "no") => invoke<boolean>("auto_checkin_answer", { id, answer }),
  contract: () => invoke<Contract>("auto_contract"),
  audit: (limit = 200) => invoke<AuditEntry[]>("auto_audit", { limit }),
  auditClear: () => invoke<void>("auto_audit_clear"),
  auditExport: (format: "csv" | "md") => invoke<string>("auto_audit_export", { format }),
  stopKeyCheck: (keys: string) => invoke<void>("auto_stop_key_check", { keys }),
  macroStart: () => invoke<void>("macro_start"),
  macroStop: () => invoke<Recording>("macro_stop"),
  macroCancel: () => invoke<void>("macro_cancel"),
  macroActive: () => invoke<boolean>("macro_active"),
};

// ------------------------------------------------------------------ budgets with an "Unlimited" choice

export interface BudgetOption {
  value: string;
  label: string;
}

const unlimited: BudgetOption = { value: "unlimited", label: "Unlimited" };

export const TIME_OPTIONS: BudgetOption[] = [
  { value: "15m", label: "15 minutes" },
  { value: "30m", label: "30 minutes" },
  { value: "1h", label: "1 hour" },
  { value: "2h", label: "2 hours" },
  { value: "4h", label: "4 hours" },
  { value: "8h", label: "8 hours" },
  unlimited,
];

export const STEP_OPTIONS: BudgetOption[] = [
  { value: "50", label: "50 steps" },
  { value: "200", label: "200 steps" },
  { value: "500", label: "500 steps" },
  { value: "1000", label: "1000 steps" },
  unlimited,
];

export const MESSAGE_OPTIONS: BudgetOption[] = [
  { value: "1", label: "1 message" },
  { value: "5", label: "5 messages" },
  { value: "10", label: "10 messages" },
  { value: "25", label: "25 messages" },
  unlimited,
];

export const STAGE_OPTIONS: BudgetOption[] = [
  { value: "10", label: "10 stages" },
  { value: "25", label: "25 stages" },
  { value: "50", label: "50 stages" },
  unlimited,
];

/** Every list of budget choices ends with Unlimited. */
export function hasUnlimited(options: BudgetOption[]): boolean {
  return options[options.length - 1]?.value === "unlimited";
}

export function emptyContract(): Contract {
  return { apps: [], folders: [], mayMessage: [], neverDelete: true, neverSpend: true, timeBudget: "30m", stepBudget: "200", messageBudget: "5" };
}

/** Names typed as "a, b; c" become a tidy list without repeats. */
export function splitList(text: string): string[] {
  const out: string[] = [];
  for (const raw of text.split(/[,;\n]/)) {
    const t = raw.trim();
    if (t && !out.some((o) => o.toLowerCase() === t.toLowerCase())) out.push(t);
  }
  return out;
}

/** What is missing before a job can start, or null. The Rust side checks again. */
export function jobProblem(request: string, stages: StageDraft[], c: Contract): string | null {
  if (!request.trim()) return "Say what the job is.";
  if (stages.length === 0) return "Split the job into stages first.";
  if (stages.some((s) => !s.text.trim())) return "Every stage needs a few words.";
  if (c.apps.length === 0) return "Name at least one program (or the browser) the job may use.";
  return null;
}

// ------------------------------------------------------------------ cards

/** An approval card's fields without the marker field, and the rule the second button would save (empty: no second button). */
export function splitRemember(fields: { label: string; value: string }[]): { fields: { label: string; value: string }[]; rule: string } {
  const marker = fields.find((f) => f.label === REMEMBER_FIELD);
  return { fields: fields.filter((f) => f.label !== REMEMBER_FIELD), rule: marker?.value ?? "" };
}

export function statusText(v: LiveView): string {
  switch (v.status) {
    case "running":
      return v.now ? `Now: ${v.now}` : "Working";
    case "waiting":
      return "Waiting for your answer";
    case "paused":
      return v.note || "Paused";
    case "done":
      return "Finished";
    case "stopped":
      return v.note ? `Stopped: ${v.note}` : "Stopped";
    case "failed":
      return v.note ? `Stopped: ${v.note}` : "Stopped: a stage did not finish";
    default:
      return "";
  }
}

export function feedLine(f: FeedItem): string {
  const mark = f.ok === null ? "…" : f.ok ? "✓" : "✗";
  return `${mark} ${f.app ? `${f.app}: ` : ""}${f.what}${f.detail ? ` (${f.detail})` : ""}`;
}

export function canResume(j: Job): boolean {
  return j.state === "paused" || j.state === "stopped" || j.state === "failed";
}

export function minutesText(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 90) return `${s} s`;
  const m = Math.round(s / 60);
  return m < 90 ? `${m} min` : `${(m / 60).toFixed(1)} h`;
}

/** The contract with the budget that ended a job set to Unlimited, or null when the job did not stop for a budget. */
export function liftBudget(c: Contract, stopReason: string): Contract | null {
  if (/time budget/.test(stopReason)) return { ...c, timeBudget: "unlimited" };
  if (/step budget/.test(stopReason)) return { ...c, stepBudget: "unlimited" };
  if (/message budget/.test(stopReason)) return { ...c, messageBudget: "unlimited" };
  return null;
}

export const DISCLAIMER = [
  "Autonomous mode lets BYTE work through a list of jobs on this PC by itself, for as long as you allow, while you are away.",
  "BYTE can make mistakes. It may click the wrong thing, misread a page or message the wrong person. Only give it programs, folders and people you are comfortable with.",
  "A job never deletes anything, never spends money, never types passwords or card numbers, and never touches programs on your never-touch list. Everything else inside the contract you set, it does without asking.",
  "You are responsible for what it does while you are away. You can stop it at any time: the Stop button on the small window, the stop key, or Stop in BYTE.",
];
