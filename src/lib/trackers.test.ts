import { describe, expect, it } from "vitest";

import { billTotals, blankTracker, everyText, inDays, isOverdue, moneyText, parseAmount, perMonth } from "./trackers";

const now = new Date(2026, 8, 30, 15, 0).getTime(); // Wed Sep 30 2026, 3 PM

describe("trackers", () => {
  it("says when, like Rust does", () => {
    expect(inDays("2026-09-30", now)).toBe("today");
    expect(inDays("2026-10-01", now)).toBe("tomorrow");
    expect(inDays("2026-10-12", now)).toBe("in 12 days");
    expect(inDays("2026-09-27", now)).toBe("3 days ago");
    expect(inDays(null, now)).toBe("");
    expect(isOverdue({ ...blankTracker("upkeep"), next: "2026-09-29" }, now)).toBe(true);
    expect(isOverdue({ ...blankTracker("upkeep"), next: "2026-09-30" }, now)).toBe(false);
  });

  it("adds up bills per month and year", () => {
    const bill = (amount: number, cycle: "monthly" | "yearly" | "weekly" | "quarterly", currency = "USD") => ({ ...blankTracker("bill"), amount, cycle, currency });
    expect(perMonth(bill(120, "yearly"))).toBe(10);
    expect(perMonth(bill(30, "quarterly"))).toBe(10);
    const t = billTotals([bill(15.49, "monthly"), bill(139, "yearly")])!;
    expect(moneyText(t.month, t.currency)).toBe("$27.07");
    expect(moneyText(t.year, t.currency)).toBe("$324.88");
    expect(billTotals([bill(10, "monthly"), bill(10, "monthly", "EUR")])).toBeNull();
    expect(billTotals([])).toBeNull();
  });

  it("reads intervals and amounts", () => {
    expect(everyText({ everyMonths: 3, everyDays: null })).toBe("every 3 months");
    expect(everyText({ everyMonths: 24, everyDays: null })).toBe("every 2 years");
    expect(everyText({ everyMonths: null, everyDays: 14 })).toBe("every 2 weeks");
    expect(parseAmount("$1,299.99")).toBe(1299.99);
    expect(parseAmount("free")).toBeNull();
    expect(moneyText(9.99, "EUR")).toBe("€9.99");
    expect(moneyText(20916.76, "USD")).toBe("$20,916.76");
    expect(moneyText(1500, "USD")).toBe("$1,500");
  });
});
