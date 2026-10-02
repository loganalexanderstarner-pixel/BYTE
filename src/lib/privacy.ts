// Settings → Privacy: labels and grouping for the activity log (privacy.rs).
import type { Activity, ActivityKind } from "./types";

export const KINDS: { id: ActivityKind; label: string }[] = [
  { id: "web", label: "Web" },
  { id: "mac", label: "Mac control" },
  { id: "terminal", label: "Terminal" },
  { id: "files", label: "Your files" },
  { id: "connectors", label: "Connectors" },
  { id: "automations", label: "Automations" },
  { id: "tasks", label: "Tasks & trackers" },
  { id: "memory", label: "Memory" },
  { id: "other", label: "Other" },
];

const TOOL_LABELS: Record<string, string> = {
  web_search: "Searched the web",
  read_page: "Read a page",
  academic_search: "Searched papers",
  find_places: "Looked up places",
  weather: "Checked the weather",
  calculate: "Calculated",
  remember: "Saved a memory",
  search_my_files: "Searched your files",
  feed_follow: "Followed a feed",
  watch_add: "Watched a page",
  open_url: "Opened a page (web agent)",
  look_at_page: "Looked at a page (web agent)",
  click: "Clicked (web agent)",
  type_text: "Typed (web agent)",
  choose_option: "Chose an option (web agent)",
  scroll_page: "Scrolled (web agent)",
  go_back: "Went back (web agent)",
  download_file: "Downloaded a file (web agent)",
  save_page: "Saved a page (web agent)",
  mac_terminal: "Ran a terminal command",
  mac_storage: "Checked storage",
  mac_health: "Ran a health check",
  mac_uninstall: "Uninstalled an app",
  mac_login_remove: "Removed a login item",
  mac_find_files: "Found files",
  mac_organize: "Organized files",
  notion_create: "Added a Notion page",
  obsidian_write: "Wrote an Obsidian note",
  automation_add: "Saved an automation",
  automation_run: "Ran an automation",
  task_add: "Added a to-do",
  tracker_add: "Added a tracker",
};

/** "mac_reminder_add" → "Reminder add" when there's no friendlier name. */
export function toolLabel(tool: string): string {
  if (TOOL_LABELS[tool]) return TOOL_LABELS[tool];
  const words = tool.replace(/^mac_/, "").replace(/_/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : "Something";
}

export function kindLabel(kind: ActivityKind): string {
  return KINDS.find((k) => k.id === kind)?.label ?? "Other";
}

/** Entries grouped by local day ("Today", "Yesterday", or the date), newest first as given. */
export function byDay(list: Activity[], now = new Date()): { day: string; items: Activity[] }[] {
  const key = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  const today = key(now);
  const y = new Date(now);
  y.setDate(now.getDate() - 1);
  const yesterday = key(y);
  const out: { day: string; items: Activity[] }[] = [];
  for (const a of list) {
    const d = new Date(a.ts);
    const k = Number.isNaN(d.getTime()) ? "" : key(d);
    const day = k === today ? "Today" : k === yesterday ? "Yesterday" : Number.isNaN(d.getTime()) ? "Earlier" : d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
    const last = out[out.length - 1];
    if (last && last.day === day) last.items.push(a);
    else out.push({ day, items: [a] });
  }
  return out;
}

/** A short line about what the arguments were (a query, a URL, a title, a command). */
export function argsLine(args: unknown): string {
  if (!args || typeof args !== "object") return "";
  const a = args as Record<string, unknown>;
  for (const k of ["query", "url", "title", "command", "expression", "text", "name", "path", "app"]) {
    const v = a[k];
    if (typeof v === "string" && v.trim()) return v.trim().slice(0, 140);
  }
  return "";
}

/** How the idle-time choice reads. */
export function lockAfterLabel(minutes: number): string {
  if (minutes <= 0) return "Only when BYTE opens";
  if (minutes < 60) return `After ${minutes} minutes idle`;
  return minutes === 60 ? "After an hour idle" : `After ${minutes / 60} hours idle`;
}
