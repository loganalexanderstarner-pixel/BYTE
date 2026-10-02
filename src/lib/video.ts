// YouTube summaries (Rust `youtube::VideoCard`): timestamps, links, text and a document.

import type { DocSpec } from "./docs/spec";
import type { VideoCard } from "./types";

/** "12:34", or "1:02:03" for an hour or more. */
export function stamp(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${r}` : `${m}:${r}`;
}

export function videoLink(id: string, secs: number): string {
  return secs > 0 ? `https://youtu.be/${id}?t=${Math.floor(secs)}` : `https://youtu.be/${id}`;
}

/** The summary as plain text (Copy). */
export function videoText(v: VideoCard): string {
  const lines = [`${v.title} (${v.channel}, ${stamp(v.seconds)})`, videoLink(v.id, 0), ""];
  if (v.tldr) lines.push(v.tldr, "");
  if (v.keyPoints.length) lines.push("Key points", ...v.keyPoints.map((p) => `- [${stamp(p.start)}] ${p.text}`), "");
  if (v.chapters.length) lines.push("Chapters", ...v.chapters.map((c) => `- [${stamp(c.start)}] ${c.title}${c.summary ? `: ${c.summary}` : ""}`));
  return lines.join("\n").trim();
}

/** The summary as a document for the PDF renderer. */
export function videoDocSpec(v: VideoCard): DocSpec {
  const sections: DocSpec["sections"] = [];
  if (v.tldr) sections.push({ title: "In short", blocks: [{ type: "callout", text: v.tldr }] });
  if (v.keyPoints.length) sections.push({ title: "Key points", blocks: [{ type: "bullets", items: v.keyPoints.map((p) => `${stamp(p.start)} — ${p.text}`) }] });
  for (const c of v.chapters) sections.push({ title: `${stamp(c.start)} · ${c.title}`, blocks: c.summary ? [{ type: "paragraph", text: c.summary }] : [] });
  return {
    kind: "pdf",
    title: v.title,
    subtitle: `${v.channel} · ${stamp(v.seconds)}${v.transcribed ? " · transcribed from its audio" : v.autoCaptions ? " · from auto-generated captions" : ""}`,
    sections,
    sources: [{ n: 1, title: `${v.title} (YouTube)`, url: videoLink(v.id, 0) }],
  };
}
