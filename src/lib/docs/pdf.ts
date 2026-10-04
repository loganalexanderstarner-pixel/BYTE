import type { Content, TDocumentDefinitions } from "pdfmake/interfaces";

import { chartAsTable, chartKey, type Block, type ChartImages, type DocSpec, type DocTheme } from "./spec";

/** The pdfmake document for a DocSpec (pure, so it can be tested without fonts). */
export function pdfDefinition(original: DocSpec, theme: DocTheme, charts: ChartImages, date = new Date()): TDocumentDefinitions {
  // pdfmake rewrites the lists it's given; keep the caller's document intact
  // (the same one can be saved as PPTX or DOCX afterwards).
  const spec: DocSpec = structuredClone(original);
  const table = (columns: string[], rows: string[][]): Content => ({
    table: {
      headerRows: 1,
      widths: columns.map(() => "*"),
      body: [columns.map((c) => ({ text: c, bold: true, fillColor: theme.soft, color: theme.heading })), ...rows.map((r) => r.map((c) => ({ text: c })))],
    },
    layout: "lightHorizontalLines",
    margin: [0, 4, 0, 12],
  });

  const block = (b: Block, key: string): Content => {
    switch (b.type) {
      case "paragraph":
        return { text: b.text, margin: [0, 0, 0, 10] };
      case "bullets":
        return { ul: b.items, margin: [0, 0, 0, 10] };
      case "numbered":
        return { ol: b.items, margin: [0, 0, 0, 10] };
      case "table":
        return table(b.columns, b.rows);
      case "chart":
        if (charts[key]) return { image: charts[key], width: 440, alignment: "center", margin: [0, 6, 0, 14] };
        {
          const t = chartAsTable(b);
          return table(t.columns, t.rows);
        }
      case "callout":
        return {
          table: { widths: ["*"], body: [[{ text: b.text, margin: [10, 8, 10, 8], color: theme.heading }]] },
          layout: { fillColor: theme.soft, hLineWidth: () => 0, vLineWidth: (i: number) => (i === 0 ? 3 : 0), vLineColor: theme.accent },
          margin: [0, 4, 0, 14],
        };
      case "quote":
        return { text: b.text, italics: true, color: theme.muted, margin: [18, 2, 18, 12] };
    }
  };

  const body: Content[] = [
    // Cover
    { canvas: [{ type: "rect", x: 0, y: 0, w: 80, h: 6, color: theme.accent }], margin: [0, 180, 0, 18] },
    { text: spec.title, style: "cover" },
    ...(spec.subtitle ? [{ text: spec.subtitle, style: "coverSub" } as Content] : []),
    { text: date.toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" }), color: theme.muted, margin: [0, 18, 0, 0] },
    { text: "", pageBreak: "after" },
    // Contents
    { toc: { title: { text: "Contents", style: "h1" } } },
    { text: "", pageBreak: "after" },
  ];
  spec.sections.forEach((s, si) => {
    body.push({ text: s.title, style: "h1", tocItem: true, ...(si > 0 ? { margin: [0, 18, 0, 8] } : {}) });
    s.blocks.forEach((b, bi) => body.push(block(b, chartKey(si, bi))));
  });
  if (spec.sources.length) {
    body.push({ text: "Sources", style: "h1", tocItem: true, margin: [0, 18, 0, 8] });
    body.push({ ol: spec.sources.map((x) => ({ text: [x.title || x.url, { text: `\n${x.url}`, color: theme.muted, fontSize: 9 }] })), fontSize: 10 });
  }

  return {
    pageSize: "LETTER",
    pageMargins: [60, 64, 60, 64],
    info: { title: spec.title, creator: "BYTE" },
    content: body,
    footer: (page: number, pages: number) =>
      page === 1 ? null : { text: `${spec.title}  ·  ${page} / ${pages}`, alignment: "center", fontSize: 8, color: theme.muted, margin: [0, 24, 0, 0] },
    defaultStyle: { fontSize: 11, lineHeight: 1.35, color: theme.text },
    styles: {
      cover: { fontSize: 32, bold: true, color: theme.heading },
      coverSub: { fontSize: 15, color: theme.muted, margin: [0, 8, 0, 0] },
      h1: { fontSize: 18, bold: true, color: theme.accent, margin: [0, 0, 0, 8] },
    },
  };
}

/** PDF bytes as base64 (loads pdfmake and its fonts on first use). */
export async function makePdf(spec: DocSpec, theme: DocTheme, charts: ChartImages): Promise<string> {
  const pdfMake = (await import("pdfmake/build/pdfmake")).default;
  const vfs = (await import("pdfmake/build/vfs_fonts")).default;
  pdfMake.addVirtualFileSystem(vfs);
  return pdfMake.createPdf(pdfDefinition(spec, theme, charts)).getBase64();
}
