import { describe, expect, it } from "vitest";
import { leftText } from "./timerLeft";

describe("leftText", () => {
  it("counts minutes and seconds", () => expect(leftText(272_000, 0)).toBe("4:32"));
  it("adds hours when there are any", () => expect(leftText(3_730_000, 0)).toBe("1:02:10"));
  it("rounds a part second up so it never shows 0:00 early", () => expect(leftText(1_200, 0)).toBe("0:02"));
  it("never goes negative", () => expect(leftText(0, 5_000)).toBe("0:00"));
});
