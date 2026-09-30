/** Helpers for the ✅ Tasks panel (Rust `tasks.rs`, `scheduler.rs`). */
import type { Task } from "./types";

const DAY = 86_400_000;

function startOfDay(t: number): number {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function clock(t: number): string {
  return new Date(t).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** "Overdue · Mon 9:00 AM", "Today 5:00 PM", "Tomorrow 9:00 AM", "Fri, Oct 2". */
export function dueText(due: number | null | undefined, now: number): string {
  if (due == null) return "";
  const day = Math.round((startOfDay(due) - startOfDay(now)) / DAY);
  const when =
    day === 0
      ? `Today ${clock(due)}`
      : day === 1
        ? `Tomorrow ${clock(due)}`
        : day > 1 && day < 7
          ? `${new Date(due).toLocaleDateString(undefined, { weekday: "short" })} ${clock(due)}`
          : new Date(due).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
  return due < now ? `Overdue · ${when}` : when;
}

/** Open tasks first (overdue and soonest first, then undated by age), then done ones, latest first. */
export function sortTasks(tasks: Task[]): Task[] {
  const open = tasks.filter((t) => t.doneAt == null);
  const done = tasks.filter((t) => t.doneAt != null);
  open.sort((a, b) => {
    if (a.due != null && b.due != null) return a.due - b.due;
    if (a.due != null) return -1;
    if (b.due != null) return 1;
    return a.created - b.created;
  });
  done.sort((a, b) => (b.doneAt ?? 0) - (a.doneAt ?? 0));
  return [...open, ...done];
}

/** A date and time input's value ("2026-09-30T17:00") → ms, or null. */
export function fromInput(value: string): number | null {
  if (!value) return null;
  const t = new Date(value).getTime();
  return Number.isNaN(t) ? null : t;
}

/** ms → a date and time input's value, in local time. */
export function toInput(ms: number | null | undefined): string {
  if (ms == null) return "";
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Topics typed as "AI, Steelers, climate" → a clean list (no blanks, no repeats, at most 5). */
export function topicList(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(",")) {
    const t = raw.trim();
    if (!t || seen.has(t.toLowerCase())) continue;
    seen.add(t.toLowerCase());
    out.push(t);
  }
  return out.slice(0, 5);
}
