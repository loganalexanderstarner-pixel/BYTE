import { Chart, registerables } from "chart.js";

import { chartKey, type ChartImages, type DocSpec, type DocTheme } from "./spec";

let registered = false;

/** Draws every chart in the document as a PNG (data URL), keyed by `chartKey`.
 * Browser only (Chart.js needs a canvas). */
export async function renderCharts(spec: DocSpec, theme: DocTheme): Promise<ChartImages> {
  if (!registered) {
    Chart.register(...registerables);
    registered = true;
  }
  const out: ChartImages = {};
  const palette = [theme.accent, "#F59E0B", "#10B981", "#EF4444", "#8B5CF6", "#06B6D4", "#EC4899", "#84CC16"];
  spec.sections.forEach((s, si) =>
    s.blocks.forEach((b, bi) => {
      if (b.type !== "chart") return;
      const canvas = document.createElement("canvas");
      canvas.width = 1200;
      canvas.height = 700;
      const chart = new Chart(canvas, {
        type: b.chart,
        data: {
          labels: b.labels,
          datasets: [
            {
              label: b.title,
              data: b.values,
              backgroundColor: b.chart === "pie" ? palette : theme.accent,
              borderColor: b.chart === "line" ? theme.accent : "#FFFFFF",
              borderWidth: b.chart === "line" ? 4 : 1,
            },
          ],
        },
        options: {
          responsive: false,
          animation: false,
          devicePixelRatio: 1,
          plugins: {
            legend: { display: b.chart === "pie", labels: { color: theme.text, font: { size: 22 } } },
            title: { display: !!b.title, text: b.title, color: theme.heading, font: { size: 30, weight: "bold" } },
          },
          scales: b.chart === "pie" ? {} : { x: { ticks: { color: theme.text, font: { size: 20 } } }, y: { ticks: { color: theme.text, font: { size: 20 } } } },
        },
        plugins: [
          {
            // White background (PNG is transparent otherwise; dark slides would hide the text).
            id: "bg",
            beforeDraw: (c) => {
              const ctx = c.canvas.getContext("2d")!;
              ctx.save();
              ctx.fillStyle = "#FFFFFF";
              ctx.fillRect(0, 0, c.width, c.height);
              ctx.restore();
            },
          },
        ],
      });
      out[chartKey(si, bi)] = canvas.toDataURL("image/png");
      chart.destroy();
    }),
  );
  return out;
}
