import { describe, expect, it } from "vitest";

import { ago, money, parseTarget, watchSummary } from "./watch";
import type { Watcher } from "./types";

const w = (over: Partial<Watcher>): Watcher => ({
  id: 1,
  url: "https://example.com/p",
  name: "example.com",
  kind: "price",
  target: null,
  everyHours: 6,
  enabled: true,
  created: 0,
  lastChecked: null,
  nextCheck: null,
  lastPrice: null,
  currency: "USD",
  lastChange: null,
  lastNote: "",
  lastError: "",
  ...over,
});

describe("watch helpers", () => {
  it("formats money like Rust does", () => {
    expect(money(179, "USD")).toBe("$179");
    expect(money(189.5, "")).toBe("$189.50");
    expect(money(50, "EUR")).toBe("€50");
    expect(money(12.5, "CAD")).toBe("12.50 CAD");
  });

  it("sums up a watcher", () => {
    expect(watchSummary(w({ target: 150 }))).toBe("Price at or below $150 · every 6 hours");
    expect(watchSummary(w({}))).toBe("Price drops · every 6 hours");
    expect(watchSummary(w({ kind: "change", everyHours: 24 }))).toBe("Changes · once a day");
    expect(watchSummary(w({ kind: "change", everyHours: 1 }))).toBe("Changes · every hour");
  });

  it("says how long ago", () => {
    const now = 10 * 86_400_000;
    expect(ago(null, now)).toBe("");
    expect(ago(now - 10_000, now)).toBe("just now");
    expect(ago(now - 5 * 60_000, now)).toBe("5m ago");
    expect(ago(now - 3 * 3_600_000, now)).toBe("3h ago");
    expect(ago(now - 2 * 86_400_000, now)).toBe("2d ago");
  });

  it("reads typed targets", () => {
    expect(parseTarget("$1,299.99")).toBe(1299.99);
    expect(parseTarget("150")).toBe(150);
    expect(parseTarget("")).toBeNull();
    expect(parseTarget("cheap")).toBeNull();
    expect(parseTarget("0")).toBeNull();
  });
});
