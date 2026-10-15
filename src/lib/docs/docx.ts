import {
  AlignmentType,
  BorderStyle,
  Document,
  HeadingLevel,
  ImageRun,
  LevelFormat,
  Packer,
  Paragraph,
  ShadingType,
  Table,
  TableCell,
  TableOfContents,
  TableRow,
  TextRun,
  WidthType,
} from "docx";

import { chartAsTable, chartKey, type Block, type ChartImages, type DocSpec, type DocTheme } from "./spec";

const hex = (c: string) => c.replace("#", "");

function base64Bytes(dataUrl: string): Uint8Array {
  const b64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Word bytes as base64: title page, table of contents (filled in when Word
 * opens it), headings, lists, tables, chart images, callouts, sources. */
export async function makeDocx(spec: DocSpec, theme: DocTheme, charts: ChartImages): Promise<string> {
  const table = (columns: string[], rows: string[][]) =>
    new Table({
      width: { size: 100, type: WidthType.PERCENTAGE },
      rows: [
        new TableRow({
          tableHeader: true,
          children: columns.map(
            (c) =>
              new TableCell({
                shading: { type: ShadingType.CLEAR, fill: hex(theme.soft), color: "auto" },
                children: [new Paragraph({ children: [new TextRun({ text: c, bold: true, color: hex(theme.heading) })] })],
              }),
          ),
        }),
        ...rows.map((r) => new TableRow({ children: r.map((c) => new TableCell({ children: [new Paragraph(c)] })) })),
      ],
    });

  const block = (b: Block, key: string): (Paragraph | Table)[] => {
    switch (b.type) {
      case "paragraph":
        return [new Paragraph({ text: b.text, spacing: { after: 160 } })];
      case "bullets":
        return b.items.map((t) => new Paragraph({ text: t, bullet: { level: 0 } }));
      case "numbered":
        return b.items.map((t) => new Paragraph({ text: t, numbering: { reference: "numbers", level: 0 } }));
      case "table":
        return [table(b.columns, b.rows), new Paragraph("")];
      case "chart":
        if (charts[key])
          return [
            new Paragraph({
              alignment: AlignmentType.CENTER,
              children: [new ImageRun({ type: "png", data: base64Bytes(charts[key]), transformation: { width: 560, height: 327 } })],
            }),
          ];
        {
          const t = chartAsTable(b);
          return [table(t.columns, t.rows), new Paragraph("")];
        }
      case "callout":
        return [
          new Paragraph({
            children: [new TextRun({ text: b.text, color: hex(theme.heading) })],
            shading: { type: ShadingType.CLEAR, fill: hex(theme.soft), color: "auto" },
            border: { left: { style: BorderStyle.SINGLE, size: 24, color: hex(theme.accent), space: 8 } },
            spacing: { before: 120, after: 200 },
          }),
        ];
      case "quote":
        return [new Paragraph({ children: [new TextRun({ text: b.text, italics: true, color: hex(theme.muted) })], indent: { left: 540, right: 540 }, spacing: { after: 160 } })];
    }
  };

  const body: (Paragraph | Table | TableOfContents)[] = [
    new Paragraph({ text: spec.title, heading: HeadingLevel.TITLE, spacing: { before: 2400 } }),
    ...(spec.subtitle ? [new Paragraph({ children: [new TextRun({ text: spec.subtitle, size: 30, color: hex(theme.muted) })] })] : []),
    new Paragraph({ children: [new TextRun({ text: new Date().toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" }), color: hex(theme.muted) })], pageBreakBefore: false }),
    new Paragraph({ text: "Contents", heading: HeadingLevel.HEADING_1, pageBreakBefore: true }),
    new TableOfContents("Contents", { hyperlink: true, headingStyleRange: "1-1" }),
  ];
  spec.sections.forEach((s, si) => {
    body.push(new Paragraph({ text: s.title, heading: HeadingLevel.HEADING_1, pageBreakBefore: si === 0 }));
    s.blocks.forEach((b, bi) => body.push(...block(b, chartKey(si, bi))));
  });
  if (spec.sources.length) {
    body.push(new Paragraph({ text: "Sources", heading: HeadingLevel.HEADING_1 }));
    spec.sources.forEach((x) => body.push(new Paragraph({ children: [new TextRun(`[${x.n}] ${x.title || x.url} — `), new TextRun({ text: x.url, color: hex(theme.accent) })] })));
  }

  const doc = new Document({
    creator: "BYTE",
    title: spec.title,
    features: { updateFields: true },
    styles: {
      default: { document: { run: { font: theme.font, size: 22, color: hex(theme.text) } } },
      paragraphStyles: [
        { id: "Title", name: "Title", basedOn: "Normal", run: { size: 56, bold: true, color: hex(theme.heading) } },
        { id: "Heading1", name: "Heading 1", basedOn: "Normal", next: "Normal", run: { size: 32, bold: true, color: hex(theme.accent) }, paragraph: { spacing: { before: 360, after: 160 } } },
      ],
    },
    numbering: { config: [{ reference: "numbers", levels: [{ level: 0, format: LevelFormat.DECIMAL, text: "%1.", alignment: AlignmentType.START }] }] },
    sections: [{ children: body }],
  });
  return Packer.toBase64String(doc);
}
