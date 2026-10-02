// Calendar files (RFC 5545 .ics) for trip plans: open the file and Calendar
// (or Outlook, or GNOME Calendar) adds the events. No permission needed.

import type { TripPlan } from "./types";

/** Escapes text per RFC 5545 (backslash, semicolon, comma, newlines). */
export function icsEscape(s: string): string {
  return s.replace(/\\/g, "\\\\").replace(/;/g, "\;").replace(/,/g, "\\,").replace(/\r?\n/g, "\\n");
}

/** Folds lines longer than 75 octets (continuation lines start with a space). */
export function icsFold(line: string): string {
  const bytes = new TextEncoder().encode(line);
  if (bytes.length <= 75) return line;
  const out: string[] = [];
  let cur = "";
  let curLen = 0;
  for (const ch of line) {
    const n = new TextEncoder().encode(ch).length;
    const limit = out.length === 0 ? 75 : 74;
    if (curLen + n > limit) {
      out.push(cur);
      cur = "";
      curLen = 0;
    }
    cur += ch;
    curLen += n;
  }
  out.push(cur);
  return out.join("\r\n ");
}

const pad = (n: number) => String(n).padStart(2, "0");
const ymd = (d: string) => d.replace(/-/g, "");

/** "09:30" → [9, 30]; "Morning" → [9, 0]; "Afternoon" → [14, 0]; "Evening" → [19, 0]. */
export function startTime(time: string): [number, number] | null {
  const m = time.match(/^(\d{1,2})[:.](\d{2})/);
  if (m) return [Math.min(23, Number(m[1])), Math.min(59, Number(m[2]))];
  const t = time.toLowerCase();
  if (t.includes("morning")) return [9, 0];
  if (t.includes("afternoon")) return [14, 0];
  if (t.includes("lunch") || t.includes("noon")) return [12, 30];
  if (t.includes("evening") || t.includes("dinner")) return [19, 0];
  if (t.includes("night")) return [21, 0];
  return null;
}

/**
 * The plan as an .ics file: one event per item (90 minutes, floating local
 * time) on the trip's dates. Without dates, returns null (nothing to schedule).
 */
export function tripIcs(plan: TripPlan, stamp: Date = new Date()): string | null {
  if (!plan.days.some((d) => d.date)) return null;
  const dtstamp = `${stamp.getUTCFullYear()}${pad(stamp.getUTCMonth() + 1)}${pad(stamp.getUTCDate())}T${pad(stamp.getUTCHours())}${pad(stamp.getUTCMinutes())}${pad(stamp.getUTCSeconds())}Z`;
  const lines = ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//BYTE//Trip planner//EN", "CALSCALE:GREGORIAN", `X-WR-CALNAME:${icsEscape(`Trip: ${plan.destination}`)}`];
  plan.days.forEach((day, i) => {
    if (!day.date) return;
    day.items.forEach((it, j) => {
      const t = startTime(it.time);
      const uid = `byte-trip-${ymd(day.date!)}-${i}-${j}-${plan.destination.toLowerCase().replace(/[^a-z0-9]+/g, "-")}@byte.local`;
      lines.push("BEGIN:VEVENT", `UID:${uid}`, `DTSTAMP:${dtstamp}`);
      if (t) {
        const [h, m] = t;
        const end = h * 60 + m + 90;
        lines.push(`DTSTART:${ymd(day.date!)}T${pad(h)}${pad(m)}00`);
        lines.push(`DTEND:${ymd(day.date!)}T${pad(Math.min(23, Math.floor(end / 60)))}${pad(end >= 24 * 60 ? 59 : end % 60)}00`);
      } else {
        lines.push(`DTSTART;VALUE=DATE:${ymd(day.date!)}`);
      }
      lines.push(`SUMMARY:${icsEscape(it.title)}`);
      if (it.place) lines.push(`LOCATION:${icsEscape(`${it.place}, ${plan.destination}`)}`);
      const desc = [it.note, it.cost != null ? `Estimated cost: ${Math.round(it.cost)} ${plan.currency}` : ""].filter(Boolean).join("\n");
      if (desc) lines.push(`DESCRIPTION:${icsEscape(desc)}`);
      lines.push("END:VEVENT");
    });
  });
  lines.push("END:VCALENDAR");
  return lines.map(icsFold).join("\r\n") + "\r\n";
}
