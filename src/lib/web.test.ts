import { describe, expect, it } from "vitest";

import { nextWeb, webState } from "./web";

describe("web pill", () => {
  it("reads the stored settings", () => {
    expect(webState({ webSearch: false, webMode: "always" })).toBe("off");
    expect(webState({ webSearch: true })).toBe("auto");
    expect(webState({ webSearch: true, webMode: "always" })).toBe("always");
    expect(webState(null)).toBe("off");
  });

  it("cycles Off → Auto → Always → Off", () => {
    let s = { webSearch: false, webMode: "auto" as const } as { webSearch: boolean; webMode?: "auto" | "always" };
    const seen: string[] = [];
    for (let i = 0; i < 3; i++) {
      s = { ...s, ...nextWeb(webState(s)) };
      seen.push(webState(s));
    }
    expect(seen).toEqual(["auto", "always", "off"]);
  });
});
