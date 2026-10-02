/** Documents made on this Mac: the shapes Rust's `docs.rs` sends (DocSpec),
 * and the document themes. Renderers turn a DocSpec into PDF / PPTX / DOCX. */

export type DocKind = "pdf" | "pptx" | "docx";

export interface OutlineSection {
  title: string;
  notes: string;
}

export interface DocOutline {
  title: string;
  subtitle: string;
  sections: OutlineSection[];
}

export type Block =
  | { type: "paragraph"; text: string }
  | { type: "bullets"; items: string[] }
  | { type: "numbered"; items: string[] }
  | { type: "table"; columns: string[]; rows: string[][] }
  | { type: "chart"; chart: "bar" | "line" | "pie"; title: string; labels: string[]; values: number[] }
  | { type: "callout"; text: string }
  | { type: "quote"; text: string };

export interface DocSection {
  title: string;
  blocks: Block[];
}

export interface DocSource {
  n: number;
  title: string;
  url: string;
}

export interface DocSpec {
  kind: DocKind;
  title: string;
  subtitle: string;
  sections: DocSection[];
  sources: DocSource[];
}

/** Rust `docs::DocEvent`. */
export type DocEvent = { kind: "phase"; text: string } | { kind: "section"; index: number; total: number; title: string };

/** Chart images (PNG data URLs) keyed by `chartKey`. */
export type ChartImages = Record<string, string>;
export const chartKey = (section: number, block: number) => `${section}-${block}`;

/**
 * Document themes. These colour the files BYTE makes (not the app, whose
 * colours come from styles/tokens.css), so they're plain hex values that
 * print well and read on white paper; slides may use a dark background.
 */
export interface DocTheme {
  id: string;
  label: string;
  accent: string;
  heading: string;
  text: string;
  muted: string;
  /** Callout and table-header background. */
  soft: string;
  slideBg: string;
  slideText: string;
  font: string;
}

export const DOC_THEMES: DocTheme[] = [
  { id: "midnight", label: "Midnight", accent: "#3D7BFF", heading: "#0F172A", text: "#1F2937", muted: "#6B7280", soft: "#EAF1FF", slideBg: "#0B1020", slideText: "#E6EBFF", font: "Helvetica" },
  { id: "clean", label: "Clean", accent: "#0F766E", heading: "#111827", text: "#1F2937", muted: "#6B7280", soft: "#E6F4F2", slideBg: "#FFFFFF", slideText: "#111827", font: "Helvetica" },
  { id: "paper", label: "Paper", accent: "#96592A", heading: "#3B2F25", text: "#3B2F25", muted: "#7A6A5A", soft: "#F5EBDD", slideBg: "#F7F1E6", slideText: "#3B2F25", font: "Georgia" },
  { id: "academic", label: "Academic", accent: "#1E3A8A", heading: "#111111", text: "#222222", muted: "#555555", soft: "#EEF1F8", slideBg: "#FFFFFF", slideText: "#111111", font: "Times New Roman" },
];

export const themeById = (id: string): DocTheme => DOC_THEMES.find((t) => t.id === id) ?? DOC_THEMES[0];

/** A safe file name from a title. */
export function fileName(title: string, kind: DocKind): string {
  const base = title.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 80) || "Document";
  return `${base}.${kind}`;
}

/** Cells of a chart shown as a table (when a chart image isn't available). */
export function chartAsTable(b: Extract<Block, { type: "chart" }>): { columns: string[]; rows: string[][] } {
  return { columns: [b.title || "Item", "Value"], rows: b.labels.map((l, i) => [l, String(b.values[i] ?? "")]) };
}

/** Splits a list into pieces of at most `n` (long bullet lists → several slides). */
export function chunks<T>(items: T[], n: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < items.length; i += n) out.push(items.slice(i, i + n));
  return out.length ? out : [[]];
}
