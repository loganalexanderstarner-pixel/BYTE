import type { AutoStep, Automation, StepRun } from "./types";
import { osText } from "../lib/platform";

/** The kinds of step the builder offers, in menu order. */
export const STEP_KINDS: { type: AutoStep["type"]; label: string; hint: string }[] = [
  { type: "ask", label: "Ask BYTE", hint: "A question or instruction. “it” means the text from the step before." },
  { type: "briefing", label: "Daily briefing", hint: "Calendar, reminders, to-dos, weather and news." },
  { type: "notify", label: "Notification", hint: osText("A Mac notification with the text so far.") },
  { type: "addTask", label: "Add a to-do", hint: "Leave empty to use the first line of the text so far." },
  { type: "saveFile", label: "Save to a file", hint: "A Markdown file in Documents → BYTE → Automations." },
  { type: "shortcut", label: "Run a Shortcut", hint: "One of your Shortcuts, given the text so far (its output goes on)." },
];

/** A new step of a kind, with sensible blanks. */
export function newStep(type: AutoStep["type"], name = ""): AutoStep {
  switch (type) {
    case "ask":
      return { type, prompt: "" };
    case "briefing":
      return { type };
    case "notify":
      return { type, title: name, body: "{previous}" };
    case "addTask":
      return { type, title: "{previous}" };
    case "saveFile":
      return { type, name };
    case "shortcut":
      return { type, name: "" };
  }
}

/** What a step does, in words (like Rust's `Step::label`). */
export function stepLabel(s: AutoStep): string {
  switch (s.type) {
    case "ask":
      return `Ask BYTE: ${s.prompt.trim() || "…"}`;
    case "briefing":
      return "Write your daily briefing";
    case "notify":
      return "Send you a notification";
    case "addTask":
      return s.title.includes("{previous}") || !s.title.trim() ? "Add it to your to-do list" : `Add “${s.title.trim()}” to your to-do list`;
    case "saveFile":
      return `Save it as “${s.name.trim() || "…"}”`;
    case "shortcut":
      return `Run your shortcut “${s.name.trim() || "…"}”`;
  }
}

/** Why the builder can't save yet (null when it can). */
export function problem(a: Pick<Automation, "name" | "trigger" | "steps">): string | null {
  if (!a.name.trim()) return "Give it a name.";
  if (!a.trigger) return "Say when it should run.";
  if (a.steps.length === 0) return "Add at least one step.";
  if (a.steps.length > 8) return "Up to 8 steps.";
  for (const [i, s] of a.steps.entries()) {
    if (s.type === "ask" && !s.prompt.trim()) return `Step ${i + 1} needs a question.`;
    if (s.type === "shortcut" && !s.name.trim()) return `Step ${i + 1} needs the shortcut's name.`;
    if (i === 0 && (s.type === "notify" || s.type === "saveFile" || s.type === "addTask") && (s.type !== "addTask" || s.title.includes("{previous}")))
      return "The first step needs to make something first (ask BYTE, the briefing or a Shortcut).";
  }
  return null;
}

/** Moves step `i` up (-1) or down (+1); unchanged at the ends. */
export function moveStep<T>(steps: T[], i: number, by: -1 | 1): T[] {
  const j = i + by;
  if (j < 0 || j >= steps.length) return steps;
  const out = steps.slice();
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}

/** Where a run is: "Step 2 of 4", "Done", "Stopped at step 3". */
export function runStatus(steps: StepRun[], finished: boolean): string {
  const failed = steps.findIndex((s) => s.status === "failed");
  if (failed >= 0) return `Stopped at step ${failed + 1}`;
  if (finished) return "Done";
  const running = steps.findIndex((s) => s.status === "running");
  if (running >= 0) return `Step ${running + 1} of ${steps.length}`;
  return steps.every((s) => s.status === "done") ? "Done" : "Starting…";
}

/** The first failed step (to run again from there), or null. */
export function failedAt(steps: StepRun[]): number | null {
  const i = steps.findIndex((s) => s.status === "failed");
  return i >= 0 ? i : null;
}

export const blankAutomation = (): Automation => ({
  id: 0,
  name: "",
  trigger: "manual",
  steps: [newStep("ask")],
  enabled: true,
  lastRun: null,
  nextRun: null,
  when: "",
  lastOk: null,
  lastChat: null,
  linked: false,
});
