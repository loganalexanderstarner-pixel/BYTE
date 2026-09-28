import type { ApprovalCard, SavedFile } from "./types";

/** The answer ended: the browser is closed and unanswered cards can't be answered any more. */
export function endBrowsing<M extends { approvals?: ApprovalCard[]; browsing?: boolean }>(m: M): M {
  if (!m.approvals?.some((a) => a.status === "waiting") && !m.browsing) return m;
  return { ...m, browsing: false, approvals: m.approvals?.map((a) => (a.status === "waiting" ? { ...a, status: "expired" } : a)) };
}

/** "2.4 MB", "830 KB". */
export function fileSize(bytes: number): string {
  const mb = 1024 * 1024;
  return bytes >= mb ? `${(bytes / mb).toFixed(1)} MB` : `${Math.max(1, Math.ceil(bytes / 1024))} KB`;
}

/** Saved files BYTE may open directly (the rest are only shown in Finder). */
export function canOpen(file: SavedFile): boolean {
  const ext = file.name.split(".").pop()?.toLowerCase() ?? "";
  return ["pdf", "png", "jpg", "jpeg", "gif", "webp", "md", "txt", "csv", "webarchive"].includes(ext);
}

/** What the approval card's button says. */
export function approveLabel(a: Pick<ApprovalCard, "action">): string {
  return a.action === "download" ? "Download" : a.action === "submit" ? "Submit" : "Press it";
}
