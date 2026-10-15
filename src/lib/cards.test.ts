import { describe, expect, it } from "vitest";

import { checkedText, nextHint, outOfFive, overCheapest, priceText, stockText } from "./cards";
import type { Offer } from "./types";

describe("card helpers", () => {
  it("formats prices", () => {
    expect(priceText(229.99, "USD")).toBe("$229.99");
    expect(priceText(1299, "EUR")).toBe("€1,299.00");
    expect(priceText(5, "")).toBe("5.00");
    expect(priceText(5, "XX")).toBe("5.00 XX");
  });

  it("puts ratings on a 5-star scale", () => {
    expect(outOfFive(4.7, 5)).toBe(4.5);
    expect(outOfFive(8, 10)).toBe(4);
    expect(outOfFive(3, 0)).toBe(0);
  });

  it("says when prices were checked", () => {
    const now = Date.parse("2026-09-29T12:00:00Z");
    expect(checkedText("2026-09-29T11:59:30Z", now)).toBe("Checked just now");
    expect(checkedText("2026-09-29T11:48:00Z", now)).toBe("Checked 12 min ago");
    expect(checkedText("2026-09-29T09:00:00Z", now)).toBe("Checked 3 h ago");
    expect(checkedText("nope", now)).toBe("");
  });

  it("compares offers and stock", () => {
    const o = (price: number): Offer => ({ store: "a", title: "x", price, currency: "USD", inStock: null, condition: "", url: "", n: 1 });
    expect(overCheapest(o(249.99), o(229.99))).toBe("+$20.00");
    expect(overCheapest(o(229.99), o(229.99))).toBe("");
    expect(stockText(null)).toBe("");
    expect(stockText(false)).toBe("Out of stock");
  });

  it("reveals hints one at a time", () => {
    expect(nextHint(0, 3)).toBe(1);
    expect(nextHint(3, 3)).toBe(3);
  });
});
