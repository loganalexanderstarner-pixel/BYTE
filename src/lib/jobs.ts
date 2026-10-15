/** Job tracker helpers (the panel's grouping and deadline text). */
import type { Job } from "./types";

export const STATUSES = ["saved", "applied", "interview", "offer", "rejected"] as const;
export const STATUS_LABEL: Record<string, string> = {
  saved: "Saved",
  applied: "Applied",
  interview: "Interviewing",
  offer: "Offer",
  rejected: "Closed",
};

export function blankJob(): Job {
  return { id: 0, company: "", role: "", location: "", pay: "", url: "", status: "saved", deadline: "", applied: "", summary: "", requirements: [], notes: "", updated: 0 };
}

/** Days from `today` to a YYYY-MM-DD deadline (negative: passed). */
export function daysLeft(deadline: string, today = new Date()): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(deadline);
  if (!m) return null;
  const d = Date.UTC(+m[1], +m[2] - 1, +m[3]);
  const t = Date.UTC(today.getFullYear(), today.getMonth(), today.getDate());
  return Math.round((d - t) / 86_400_000);
}

export function deadlineText(deadline: string, today = new Date()): string {
  const n = daysLeft(deadline, today);
  if (n == null) return "";
  if (n < 0) return "deadline passed";
  if (n === 0) return "apply today";
  if (n === 1) return "apply by tomorrow";
  return n <= 14 ? `${n} days left` : `apply by ${deadline}`;
}

/** Jobs grouped by status, in pipeline order, empty groups left out. */
export function groupJobs(jobs: Job[]): [string, Job[]][] {
  return STATUSES.map((s) => [s, jobs.filter((j) => (STATUSES as readonly string[]).includes(j.status) ? j.status === s : s === "saved")] as [string, Job[]]).filter(
    ([, l]) => l.length > 0,
  );
}
