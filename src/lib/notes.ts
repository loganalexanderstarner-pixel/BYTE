// Notes in the UI: filtering, tags and labels (notes.rs owns the files).
import type { Note } from "./types";

export interface NoteFilter {
  query: string;
  folder: string;
  tag: string;
}

/** Notes matching every word of the query (title, tags, text), the folder and the tag; title hits first. */
export function filterNotes(notes: Note[], f: NoteFilter): Note[] {
  const words = f.query.toLowerCase().split(/\s+/).filter(Boolean);
  return notes
    .filter((n) => !f.folder || n.folder === f.folder)
    .filter((n) => !f.tag || n.tags.includes(f.tag))
    .map((n) => {
      const title = n.title.toLowerCase();
      const all = `${title} ${n.tags.join(" ")} ${n.body.toLowerCase()}`;
      const ok = words.every((w) => all.includes(w));
      return { n, ok, score: words.filter((w) => title.includes(w)).length };
    })
    .filter((x) => x.ok)
    .sort((a, b) => b.score - a.score || b.n.updated - a.n.updated)
    .map((x) => x.n);
}

/** Tags with how many notes use them, most used first. */
export function allTags(notes: Note[]): [string, number][] {
  const m = new Map<string, number>();
  for (const n of notes) for (const t of n.tags) m.set(t, (m.get(t) ?? 0) + 1);
  return [...m.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

/** "work, #Ideas,  travel" → ["work", "ideas", "travel"] (unique). */
export function parseTags(text: string): string[] {
  return [...new Set(text.split(",").map((t) => t.trim().replace(/^#/, "").toLowerCase()).filter(Boolean))];
}

/** "just now", "5 min ago", "3 h ago", "Sep 28". */
export function noteAge(ms: number, now = Date.now()): string {
  const d = now - ms;
  if (d < 60_000) return "just now";
  if (d < 3_600_000) return `${Math.floor(d / 60_000)} min ago`;
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)} h ago`;
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** A note from a chat answer: title from the chat, the answer without citation marks, tags from the chat. */
export function noteFromAnswer(content: string, chatTitle: string, chatTags: string[] = []): { title: string; folder: string; tags: string[]; body: string } {
  return {
    title: (chatTitle || content.split("\n").find((l) => l.trim()) || "Note").replace(/^#+\s*/, "").slice(0, 80),
    folder: "Inbox",
    tags: chatTags.slice(0, 5),
    body: content.replace(/\s?\[\d{1,3}(?:,\s*\d{1,3})*\]/g, "").trim(),
  };
}

/** The bookmarklet that sends the page (and any selected text) to BYTE's clipper. */
export const BOOKMARKLET =
  "javascript:location.href='byte://clip?url='+encodeURIComponent(location.href)+'&sel='+encodeURIComponent(String(getSelection()).slice(0,20000))";
