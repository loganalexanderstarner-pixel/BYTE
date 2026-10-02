import { describe, expect, it } from "vitest";

import { icsEscape, icsFold, startTime, tripIcs } from "./ics";
import { tripDocSpec, tripTotals } from "./trip";
import type { TripPlan } from "./types";

const plan: TripPlan = {
  destination: "Lisbon, Portugal",
  currency: "EUR",
  budget: 1500,
  travelers: 2,
  month: null,
  days: [
    { title: "Alfama", date: "2026-10-03", items: [{ time: "09:00", title: "Castelo de São Jorge", place: "Alfama", note: "Go early; it gets busy", cost: 30, sources: [2] }, { time: "Evening", title: "Fado dinner", place: "Bairro Alto", note: "", cost: null, sources: [] }] },
    { title: "Belém", date: "2026-10-04", items: [{ time: "whenever", title: "Pastéis de Belém", place: "Belém", note: "", cost: 8, sources: [] }] },
  ],
  costs: [
    { category: "Lodging", amount: 600 },
    { category: "Food", amount: 400 },
  ],
  packing: ["Walking shoes"],
  tips: ["Buy a Viva Viagem card"],
  weather: "Typical: 17–24 °C.",
};

describe("trip plans", () => {
  it("totals the budget", () => {
    expect(tripTotals(plan)).toEqual({ total: 1000, left: 500 });
    expect(tripTotals({ ...plan, budget: null }).left).toBeNull();
  });

  it("becomes a document for the PDF renderer", () => {
    const spec = tripDocSpec(plan, [{ n: 2, title: "Castle", url: "https://x.pt", snippet: "", read: true }]);
    expect(spec.title).toBe("2 days in Lisbon, Portugal");
    expect(spec.subtitle).toContain("2 travellers");
    expect(spec.sections.map((s) => s.title.split(":")[0])).toEqual(["Day 1 · Sat, Oct 3", "Day 2 · Sun, Oct 4", "Budget", "Weather and packing", "Tips"]);
    const table = spec.sections[0].blocks[0];
    expect(table.type === "table" && table.rows[0][1]).toBe("Castelo de São Jorge — Go early; it gets busy [2]");
    expect(spec.sources).toEqual([{ n: 2, title: "Castle", url: "https://x.pt" }]);
    const budget = spec.sections[2].blocks;
    expect(budget.some((b) => b.type === "chart")).toBe(true);
    expect(budget.some((b) => b.type === "callout" && b.text.includes("under"))).toBe(true);
  });

  it("writes a valid calendar file", () => {
    const ics = tripIcs(plan, new Date(Date.UTC(2026, 8, 28, 12, 0, 0)))!;
    const lines = ics.split("\r\n");
    expect(lines[0]).toBe("BEGIN:VCALENDAR");
    expect(ics.endsWith("END:VCALENDAR\r\n")).toBe(true);
    expect(lines.filter((l) => l === "BEGIN:VEVENT")).toHaveLength(3);
    expect(ics).toContain("DTSTART:20261003T090000");
    expect(ics).toContain("DTEND:20261003T103000");
    expect(ics).toContain("DTSTART:20261003T190000");
    // No time: an all-day event.
    expect(ics).toContain("DTSTART;VALUE=DATE:20261004");
    expect(ics).toContain("DESCRIPTION:Go early\; it gets busy\\nEstimated cost: 30 EUR");
    expect(ics).toContain("DTSTAMP:20260928T120000Z");
    expect(lines.every((l) => new TextEncoder().encode(l).length <= 75)).toBe(true);
    expect(tripIcs({ ...plan, days: plan.days.map((d) => ({ ...d, date: null })) })).toBeNull();
  });

  it("escapes, folds and reads times", () => {
    expect(icsEscape("a,b;c\\d\ne")).toBe("a\\,b\;c\\\\d\\ne");
    const long = `SUMMARY:${"é".repeat(60)}`;
    const folded = icsFold(long).split("\r\n");
    expect(folded.length).toBe(2);
    expect(folded[1].startsWith(" ")).toBe(true);
    expect(folded.every((l) => new TextEncoder().encode(l).length <= 75)).toBe(true);
    expect(startTime("14:30")).toEqual([14, 30]);
    expect(startTime("Afternoon")).toEqual([14, 0]);
    expect(startTime("anytime")).toBeNull();
  });
});
