import JSZip from "jszip";
import { describe, expect, it } from "vitest";

import { makeDocx } from "./docx";
import { pdfDefinition } from "./pdf";
import { makePptx, planSlides } from "./pptx";
import { chunks, fileName, themeById, type DocSpec } from "./spec";

export const SAMPLE: DocSpec = {
  kind: "pdf",
  title: "Solar Power for Beginners",
  subtitle: "How panels work and what they cost",
  sections: [
    {
      title: "How solar panels work",
      blocks: [
        { type: "paragraph", text: "Solar panels turn sunlight into electricity using silicon cells [1]." },
        { type: "bullets", items: ["Sunlight frees electrons", "An inverter makes AC power", "Extra power goes to the grid", "Batteries store some", "Panels last 25+ years", "They need little care", "Clean them once a year"] },
        { type: "callout", text: "A typical home system is 6–8 kW." },
      ],
    },
    {
      title: "What it costs",
      blocks: [
        { type: "table", columns: ["Year", "Cost per watt"], rows: [["2015", "$3.00"], ["2025", "$1.10"]] },
        { type: "chart", chart: "bar", title: "Cost per watt", labels: ["2015", "2025"], values: [3, 1.1] },
        { type: "numbered", items: ["Get quotes", "Check incentives", "Compare warranties"] },
        { type: "quote", text: "The cheapest electricity is the kind you make yourself." },
      ],
    },
  ],
  sources: [{ n: 1, title: "How solar works", url: "https://example.com/solar" }],
};

const unzip = async (b64: string) => JSZip.loadAsync(Buffer.from(b64, "base64"));

describe("document renderers", () => {
  const theme = themeById("clean");

  it("makes a Word file with every block", async () => {
    const zip = await unzip(await makeDocx(SAMPLE, theme, {}));
    const xml = await zip.file("word/document.xml")!.async("string");
    for (const t of ["Solar Power for Beginners", "How solar panels work", "Clean them once a year", "Cost per watt", "Get quotes", "Sources"]) expect(xml).toContain(t);
  });

  it("makes slides: long lists split, one visual per slide, native chart", async () => {
    const plans = planSlides(SAMPLE);
    expect(plans.map((p) => p.title)).toEqual(["How solar panels work", "How solar panels work (cont.)", "What it costs", "What it costs"]);
    expect(plans[2].visual?.type).toBe("table");
    expect(plans[3].visual?.type).toBe("chart");
    const zip = await unzip(await makePptx(SAMPLE, theme));
    const slides = Object.keys(zip.files).filter((f) => /^ppt\/slides\/slide\d+\.xml$/.test(f));
    expect(slides.length).toBe(1 + plans.length + 1); // cover + content + sources
    expect(Object.keys(zip.files).some((f) => f.startsWith("ppt/charts/"))).toBe(true);
  });

  it("builds a PDF layout with cover, contents and sources", () => {
    const def = pdfDefinition(SAMPLE, theme, {}, new Date(2026, 8, 28));
    const json = JSON.stringify(def.content);
    expect(json).toContain('"toc"');
    expect(json).toContain("Solar Power for Beginners");
    expect(json).toContain("https://example.com/solar");
    // Without a chart image the chart becomes a table.
    expect(json.match(/Cost per watt/g)!.length).toBeGreaterThanOrEqual(2);
  });

  it("helpers", () => {
    expect(fileName('Plan: "Q3" / 2026', "pdf")).toBe("Plan Q3 2026.pdf");
    expect(chunks([1, 2, 3, 4, 5], 2)).toEqual([[1, 2], [3, 4], [5]]);
  });

  it("making a PDF leaves the document unchanged for the other formats", async () => {
    const before = JSON.stringify(SAMPLE);
    const { makePdf } = await import("./pdf");
    await makePdf(SAMPLE, theme, {});
    expect(JSON.stringify(SAMPLE)).toBe(before);
  }, 30_000);
});
