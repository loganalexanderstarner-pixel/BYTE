import PptxGenJS from "pptxgenjs";

import { chunks, type Block, type DocSpec, type DocTheme } from "./spec";

const hex = (c: string) => c.replace("#", "");
type TextBlock = Extract<Block, { type: "paragraph" | "bullets" | "numbered" | "callout" | "quote" }>;
type Visual = Extract<Block, { type: "table" | "chart" }>;

/** A slide's worth of a section: text on the left, one table or chart beside it. */
export interface SlidePlan {
  title: string;
  text: TextBlock[];
  visual?: Visual;
}

/** Splits sections into slides: at most 6 bullets or ~550 characters of text
 * per slide, and one table/chart per slide. Pure, so it can be tested. */
export function planSlides(spec: DocSpec): SlidePlan[] {
  const slides: SlidePlan[] = [];
  for (const s of spec.sections) {
    const text: { block: TextBlock; breakBefore: boolean }[] = [];
    const visuals: Visual[] = [];
    for (const b of s.blocks) {
      if (b.type === "table" || b.type === "chart") visuals.push(b);
      // Long lists continue on the next slide.
      else if ((b.type === "bullets" || b.type === "numbered") && b.items.length > 6) chunks(b.items, 6).forEach((items, i) => text.push({ block: { ...b, items }, breakBefore: i > 0 }));
      else text.push({ block: b, breakBefore: false });
    }
    const pages: TextBlock[][] = [];
    let cur: TextBlock[] = [];
    let size = 0;
    for (const { block: t, breakBefore } of text) {
      const n = "text" in t ? t.text.length : t.items.join(" ").length + t.items.length * 40;
      if (cur.length && (breakBefore || size + n > 550)) {
        pages.push(cur);
        cur = [];
        size = 0;
      }
      cur.push(t);
      size += n;
    }
    if (cur.length || !pages.length) pages.push(cur);
    pages.forEach((p, i) => slides.push({ title: i === 0 ? s.title : `${s.title} (cont.)`, text: p, visual: i === 0 ? visuals[0] : undefined }));
    visuals.slice(1).forEach((v) => slides.push({ title: s.title, text: [], visual: v }));
  }
  return slides;
}

/** PowerPoint bytes as base64. Charts are native PowerPoint charts. */
export async function makePptx(spec: DocSpec, theme: DocTheme): Promise<string> {
  const pres = new PptxGenJS();
  pres.layout = "LAYOUT_WIDE"; // 13.33 x 7.5 in
  pres.title = spec.title;
  pres.author = "BYTE";
  const W = 13.33;
  const fg = hex(theme.slideText);
  const accent = hex(theme.accent);
  pres.defineSlideMaster({
    title: "BYTE",
    background: { color: hex(theme.slideBg) },
    objects: [{ rect: { x: 0, y: 0, w: 0.16, h: 7.5, fill: { color: accent } } }],
    slideNumber: { x: W - 0.9, y: 7.0, fontSize: 10, color: fg },
  });

  const cover = pres.addSlide({ masterName: "BYTE" });
  cover.addShape(pres.ShapeType.rect, { x: 0.8, y: 2.55, w: 1.2, h: 0.09, fill: { color: accent } });
  cover.addText(spec.title, { x: 0.8, y: 2.75, w: W - 1.6, h: 1.4, fontSize: 40, bold: true, color: fg, fontFace: theme.font, valign: "top" });
  if (spec.subtitle) cover.addText(spec.subtitle, { x: 0.8, y: 4.2, w: W - 1.6, h: 0.8, fontSize: 20, color: fg, fontFace: theme.font, transparency: 20 });

  for (const plan of planSlides(spec)) {
    const slide = pres.addSlide({ masterName: "BYTE" });
    slide.addText(plan.title, { x: 0.6, y: 0.35, w: W - 1.2, h: 0.9, fontSize: 28, bold: true, color: fg, fontFace: theme.font });
    const hasText = plan.text.length > 0;
    const textW = plan.visual && hasText ? 6.1 : W - 1.2;
    if (hasText) {
      const runs: PptxGenJS.TextProps[] = [];
      for (const t of plan.text) {
        if (t.type === "bullets" || t.type === "numbered") {
          t.items.forEach((item) =>
            runs.push({ text: item, options: { bullet: t.type === "numbered" ? { type: "number" } : true, breakLine: true, paraSpaceAfter: 6 } }),
          );
        } else if (t.type === "callout") {
          runs.push({ text: t.text, options: { bold: true, color: accent, breakLine: true, paraSpaceAfter: 10 } });
        } else {
          runs.push({ text: t.text, options: { italic: t.type === "quote", breakLine: true, paraSpaceAfter: 10 } });
        }
      }
      slide.addText(runs, { x: 0.6, y: 1.35, w: textW, h: 5.5, fontSize: 18, color: fg, fontFace: theme.font, valign: "top", fit: "shrink" });
    }
    const v = plan.visual;
    if (v) {
      const box = hasText ? { x: 7.0, y: 1.45, w: 5.7, h: 5.2 } : { x: 0.8, y: 1.45, w: W - 1.6, h: 5.4 };
      if (v.type === "table") {
        const header = v.columns.map((c) => ({ text: c, options: { bold: true, fill: { color: accent }, color: "FFFFFF" } }));
        const rows = v.rows.slice(0, 12).map((r) => r.map((c) => ({ text: c })));
        slide.addTable([header, ...rows], { ...box, h: undefined, fontSize: 13, color: fg, border: { type: "solid", pt: 0.5, color: "999999" }, autoPage: false });
      } else {
        const type = v.chart === "pie" ? pres.ChartType.pie : v.chart === "line" ? pres.ChartType.line : pres.ChartType.bar;
        slide.addChart(type, [{ name: v.title || "Values", labels: v.labels, values: v.values }], {
          ...box,
          chartColors: [accent, "F59E0B", "10B981", "EF4444", "8B5CF6", "06B6D4"],
          showTitle: !!v.title,
          title: v.title,
          titleColor: fg,
          catAxisLabelColor: fg,
          valAxisLabelColor: fg,
          showLegend: v.chart === "pie",
          legendColor: fg,
        });
      }
    }
  }

  if (spec.sources.length) {
    const slide = pres.addSlide({ masterName: "BYTE" });
    slide.addText("Sources", { x: 0.6, y: 0.35, w: W - 1.2, h: 0.9, fontSize: 28, bold: true, color: fg, fontFace: theme.font });
    slide.addText(
      spec.sources.map((s) => ({ text: `[${s.n}] ${s.title || s.url} — ${s.url}`, options: { breakLine: true, paraSpaceAfter: 4 } })),
      { x: 0.6, y: 1.35, w: W - 1.2, h: 5.5, fontSize: 12, color: fg, valign: "top", fit: "shrink" },
    );
  }
  return (await pres.write({ outputType: "base64" })) as string;
}
