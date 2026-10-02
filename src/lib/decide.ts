// Compare & decide: weighted totals and ranking for the score table
// (Rust `decide::Decision`). Weights are the user's slider values (0–5).

import type { Decision } from "./types";

/** Weighted average score (out of 10) per option; unscored cells and zero weights don't count. */
export function weightedTotals(d: Decision, weights: number[] = d.criteria.map((c) => c.weight)): number[] {
  return d.scores.map((row) => {
    let sum = 0;
    let w = 0;
    row.forEach((cell, j) => {
      const wj = weights[j] ?? 0;
      if (cell && wj > 0) {
        sum += cell.score * wj;
        w += wj;
      }
    });
    return w > 0 ? sum / w : 0;
  });
}

/** Option indexes from best to worst total (ties keep the original order). */
export function ranking(totals: number[]): number[] {
  return totals.map((t, i) => [t, i] as const).sort((a, b) => b[0] - a[0] || a[1] - b[1]).map(([, i]) => i);
}

/** The table as Markdown (for Copy). */
export function decisionMarkdown(d: Decision, weights: number[] = d.criteria.map((c) => c.weight)): string {
  const totals = weightedTotals(d, weights);
  const head = `| Criterion (weight) | ${d.options.join(" | ")} |\n|---|${d.options.map(() => "---|").join("")}`;
  const rows = d.criteria.map((c, j) => `| ${c.name} (${weights[j]}) | ${d.scores.map((r) => (r[j] ? `${r[j]!.score}/10` : "–")).join(" | ")} |`);
  return [head, ...rows, `| **Total** | ${totals.map((t) => `**${t.toFixed(1)}**`).join(" | ")} |`].join("\n");
}
