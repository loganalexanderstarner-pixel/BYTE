import type { DashboardSummary } from "./types";

/** One home tile: what it says and what asking it does. */
export interface Tile {
  id: string;
  title: string;
  lines: string[];
  /** Asked in chat when the tile is clicked (null: opens a panel instead). */
  ask: string | null;
  tone?: "warn";
}

/** "8:00 AM" etc. for the next automation run. */
function at(ms: number, now: number): string {
  const d = new Date(ms);
  const t = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const days = Math.round((new Date(d.toDateString()).getTime() - new Date(new Date(now).toDateString()).getTime()) / 86_400_000);
  if (days === 0) return `today ${t}`;
  if (days === 1) return `tomorrow ${t}`;
  return `${d.toLocaleDateString(undefined, { weekday: "short" })} ${t}`;
}

/** The tiles worth showing (empty ones are left out, so a new user sees none). */
export function tiles(s: DashboardSummary | null, today: [string, string][] | null, now: number): Tile[] {
  const out: Tile[] = [];
  if (today && today.length > 0) {
    out.push({ id: "today", title: `Today · ${today.length} event${today.length === 1 ? "" : "s"}`, lines: today.slice(0, 3).map(([t, s]) => `${t} — ${s}`), ask: "Give me my briefing" });
  }
  if (!s) return out;
  if (s.todosDue > 0) {
    out.push({
      id: "todos",
      title: s.todosOverdue > 0 ? `To-dos · ${s.todosOverdue} overdue` : `To-dos · ${s.todosDue} due today`,
      lines: s.todos,
      ask: "What's on my to-do list?",
      tone: s.todosOverdue > 0 ? "warn" : undefined,
    });
  }
  if (s.coming.length > 0) {
    out.push({ id: "coming", title: "Coming up", lines: s.coming.map((c) => `${c.name} — ${c.when}${c.overdue ? " (overdue)" : ""}`), ask: "What's coming up?", tone: s.coming.some((c) => c.overdue) ? "warn" : undefined });
  }
  if (s.changes.length > 0 || s.unread > 0) {
    const lines = s.changes.map(([n, what]) => `${n}: ${what}`);
    if (s.unread > 0) lines.push(`${s.unread} new ${s.unread === 1 ? "story" : "stories"} in your feeds`);
    out.push({ id: "watch", title: "News and watched pages", lines: lines.slice(0, 3), ask: s.unread > 0 ? "What's new in my feeds?" : "What am I watching?" });
  }
  if (s.nextAutomation) {
    out.push({ id: "auto", title: "Next automation", lines: [`${s.nextAutomation[0]} — ${at(s.nextAutomation[1], now)}`], ask: null });
  }
  if (s.research > 0) {
    out.push({ id: "research", title: "Research library", lines: [`${s.research} chat${s.research === 1 ? "" : "s"} with sources`], ask: null });
  }
  return out;
}

/** A tiny bar chart's heights (0–1) for questions per day. */
export function bars(perDay: number[]): number[] {
  const max = Math.max(1, ...perDay);
  return perDay.map((n) => n / max);
}
