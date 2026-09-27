/**
 * Reading the BYTE cloud's replies for documents, jobs, templates and the
 * library. The contract (docs/CLOUD-MODE.md) names the endpoints but not every
 * field, so these helpers accept the common shapes and never throw.
 */

export type Row = Record<string, unknown>;

/** Items in a list reply, whatever it's wrapped in. */
export function listOf(v: unknown): Row[] {
  if (Array.isArray(v)) return v.filter((x): x is Row => !!x && typeof x === "object");
  if (v && typeof v === "object") {
    for (const k of ["items", "results", "data", "documents", "jobs", "templates", "attachments", "conversations", "memories", "knowledge", "prompts", "recipes"]) {
      const inner = (v as Row)[k];
      if (Array.isArray(inner)) return listOf(inner);
    }
  }
  return [];
}

export const str = (v: unknown): string | undefined =>
  typeof v === "string" && v.length ? v : typeof v === "number" ? String(v) : undefined;

export const idOf = (r: Row | null | undefined): string | undefined => (r ? (str(r.id) ?? str(r.job_id) ?? str(r.document_id)) : undefined);

export const titleOf = (r: Row): string =>
  str(r.title) ?? str(r.name) ?? str(r.filename) ?? str(r.heading) ?? str(r.topic) ?? str(r.prompt)?.slice(0, 80) ?? "Untitled";

/** Document kinds the cloud generates, in the order shown. */
export const DOC_KINDS = [
  { id: "pdf", label: "PDF report" },
  { id: "pptx", label: "Slides" },
  { id: "docx", label: "Word document" },
  { id: "flyer", label: "Flyer" },
  { id: "worksheet", label: "Worksheet" },
  { id: "project", label: "Project" },
] as const;
export type DocKind = (typeof DOC_KINDS)[number]["id"];

export interface JobState {
  phase: "working" | "approval" | "done" | "failed" | "rejected";
  /** 0–1 when the server says. */
  progress?: number;
  label: string;
  documentId?: string;
}

const lower = (v: unknown) => (str(v) ?? "").toLowerCase();

/** Where a document job is, from `GET /api/jobs/{id}`. */
export function jobState(job: Row): JobState {
  const status = lower(job.status ?? job.state);
  const raw = typeof job.progress === "number" ? job.progress : typeof job.percent === "number" ? job.percent : undefined;
  const progress = raw === undefined ? undefined : raw > 1 ? raw / 100 : raw;
  const label = str(job.phase) ?? str(job.message) ?? str(job.step) ?? str(job.status) ?? "Working";
  const documentId = str(job.document_id) ?? str((job.document as Row | undefined)?.id) ?? str(job.result_id);
  if (/fail|error/.test(status)) return { phase: "failed", progress, label: str(job.error) ?? str(job.message) ?? "The document couldn't be made." };
  if (/reject|cancel/.test(status)) return { phase: "rejected", progress, label: "Thrown away" };
  if (/outline|approv|await|review|pending_approval/.test(status) || job.needs_approval === true || job.awaiting_approval === true)
    return { phase: "approval", progress, label: "Outline ready to review" };
  if (/done|complete|finish|success|ready/.test(status) || documentId) return { phase: "done", progress: 1, label: "Done", documentId };
  return { phase: "working", progress, label };
}

/** One heading of an outline, keeping whatever else the server sent with it. */
export interface OutlineItem {
  title: string;
  /** Original item (an object), or null when the server sent plain strings. */
  raw: Row | null;
  key: string;
}

const TITLE_KEYS = ["title", "heading", "name", "text"] as const;

export function readOutline(v: unknown): OutlineItem[] {
  const arr = Array.isArray(v) ? v : Array.isArray((v as Row)?.outline) ? ((v as Row).outline as unknown[]) : Array.isArray((v as Row)?.sections) ? ((v as Row).sections as unknown[]) : [];
  return arr.map((item, i) => {
    if (typeof item === "string") return { title: item, raw: null, key: `o${i}` };
    const r = (item ?? {}) as Row;
    const title = TITLE_KEYS.map((k) => str(r[k])).find(Boolean) ?? "";
    return { title, raw: r, key: `o${i}` };
  });
}

/** The edited outline in the shape the server sent it. */
export function writeOutline(items: OutlineItem[]): unknown[] {
  return items.map((it) => {
    if (!it.raw) return it.title;
    const k = TITLE_KEYS.find((key) => typeof it.raw![key] === "string") ?? "title";
    return { ...it.raw, [k]: it.title };
  });
}

/** Is this attachment an image (shown as a thumbnail)? */
export function isImage(r: Row): boolean {
  const t = lower(r.content_type ?? r.mime ?? r.mime_type ?? r.kind ?? r.type);
  const n = lower(r.name ?? r.filename);
  return t.startsWith("image") || /\.(png|jpe?g|gif|webp|heic)$/.test(n);
}
