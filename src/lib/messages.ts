import type { TextMessage } from "./types";

/** "2:05 PM" today, "Tue" this week, else "Sep 30". */
export function when(ms: number, now = Date.now()): string {
  const d = new Date(ms);
  const n = new Date(now);
  if (d.toDateString() === n.toDateString()) return d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  if (now - ms < 6 * 86400000) return d.toLocaleDateString([], { weekday: "short" });
  return d.toLocaleDateString([], { month: "short", day: "numeric" });
}

/** The conversation as the model reads it for "Draft a reply": the last few texts, who said what. */
export function transcript(msgs: TextMessage[], wish = "", max = 10): string {
  const lines = msgs.slice(-max).map((m) => `${m.fromMe ? "Me" : m.sender || "Them"}: ${m.text}`);
  const want = wish.trim() ? `\n\n(What I want to say in my reply: ${wish.trim()})` : "";
  return `${lines.join("\n")}${want}`;
}
