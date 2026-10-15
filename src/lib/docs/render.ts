import type { ChartImages, DocKind, DocSpec, DocTheme } from "./spec";

/** The finished file as base64, in any of the three formats (renderers are
 * loaded only when first used: pdfmake's fonts alone are ~850 KB). */
export async function renderDoc(kind: DocKind, spec: DocSpec, theme: DocTheme, charts: ChartImages): Promise<string> {
  switch (kind) {
    case "pdf":
      return (await import("./pdf")).makePdf(spec, theme, charts);
    case "pptx":
      return (await import("./pptx")).makePptx(spec, theme);
    case "docx":
      return (await import("./docx")).makeDocx(spec, theme, charts);
  }
}
