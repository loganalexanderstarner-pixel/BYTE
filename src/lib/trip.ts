// Trip plans (Rust `trip::TripPlan`): money formatting, totals, and the plan
// as a DocSpec so the existing PDF renderer can save it.

import type { DocSpec, Block } from "./docs/spec";
import type { Source, TripPlan } from "./types";

export function money(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat(undefined, { style: "currency", currency, maximumFractionDigits: 0 }).format(amount);
  } catch {
    return `${Math.round(amount)} ${currency}`;
  }
}

/** The budget total, and what's left of the user's budget (null without one). */
export function tripTotals(plan: TripPlan): { total: number; left: number | null } {
  const total = plan.costs.reduce((s, c) => s + c.amount, 0);
  return { total, left: plan.budget != null ? plan.budget - total : null };
}

/** "Sat, Oct 3" from "2026-10-03" (local calendar date, no time zone shift). */
export function dayLabel(date: string | null): string {
  if (!date) return "";
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
}

/** The trip as a document: a section per day, then the budget, packing list and tips. */
export function tripDocSpec(plan: TripPlan, sources: Source[] = []): DocSpec {
  const n = plan.days.length;
  const when = plan.days[0]?.date ? `${dayLabel(plan.days[0].date)} – ${dayLabel(plan.days[n - 1].date)}` : plan.month ?? "";
  const people = `${plan.travelers} ${plan.travelers === 1 ? "traveller" : "travellers"}`;
  const sections: DocSpec["sections"] = plan.days.map((d, i) => ({
    title: `Day ${i + 1}${d.date ? ` · ${dayLabel(d.date)}` : ""}: ${d.title}`,
    blocks: [
      {
        type: "table",
        columns: ["When", "What", "Where", "Cost"],
        rows: d.items.map((it) => [
          it.time,
          `${it.title}${it.note ? ` — ${it.note}` : ""}${it.sources.length ? ` ${it.sources.map((s) => `[${s}]`).join("")}` : ""}`,
          it.place,
          it.cost != null ? money(it.cost, plan.currency) : "Free",
        ]),
      },
    ],
  }));
  const { total, left } = tripTotals(plan);
  const budget: Block[] = [
    { type: "table", columns: ["Category", "Estimate"], rows: [...plan.costs.map((c) => [c.category, money(c.amount, plan.currency)]), ["Total", money(total, plan.currency)]] },
  ];
  if (plan.costs.length > 1) {
    budget.push({ type: "chart", chart: "pie", title: "Where the money goes", labels: plan.costs.map((c) => c.category), values: plan.costs.map((c) => c.amount) });
  }
  if (left != null) {
    budget.push({ type: "callout", text: left >= 0 ? `About ${money(left, plan.currency)} under your ${money(plan.budget!, plan.currency)} budget.` : `About ${money(-left, plan.currency)} over your ${money(plan.budget!, plan.currency)} budget.` });
  }
  if (plan.costs.length) sections.push({ title: "Budget", blocks: budget });
  const extra: Block[] = [];
  if (plan.weather) extra.push({ type: "paragraph", text: plan.weather });
  if (plan.packing.length) extra.push({ type: "bullets", items: plan.packing });
  if (extra.length) sections.push({ title: "Weather and packing", blocks: extra });
  if (plan.tips.length) sections.push({ title: "Tips", blocks: [{ type: "numbered", items: plan.tips }, { type: "callout", text: "Prices and opening hours are estimates; check them before booking." }] });
  return {
    kind: "pdf",
    title: `${n} ${n === 1 ? "day" : "days"} in ${plan.destination}`,
    subtitle: [when, people].filter(Boolean).join(" · "),
    sections,
    sources: sources.map((s) => ({ n: s.n, title: s.title, url: s.url })),
  };
}
