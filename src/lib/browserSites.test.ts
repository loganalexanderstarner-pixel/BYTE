import { describe, expect, it } from "vitest";

import { addSite, clampSteps, cleanSite, removeSite } from "./browserSites";

describe("browser site lists", () => {
  it("cleans what people type", () => {
    expect(cleanSite("https://www.Example.com/a/b?c")).toBe("example.com");
    expect(cleanSite("*.news.example.org")).toBe("news.example.org");
    expect(cleanSite("example.com:8080")).toBe("example.com");
    expect(cleanSite("nothing")).toBe("");
    expect(cleanSite("")).toBe("");
  });
  it("adds without duplicates and removes", () => {
    const a = addSite([], "www.docs.rs");
    expect(addSite(a, "https://docs.rs/x")).toEqual(["docs.rs"]);
    expect(addSite(a, "bad")).toEqual(["docs.rs"]);
    expect(removeSite(a, "docs.rs")).toEqual([]);
  });
  it("clamps the step limit", () => {
    expect(clampSteps(500)).toBe(100);
    expect(clampSteps(-3)).toBe(0);
    expect(clampSteps(NaN)).toBe(0);
    expect(clampSteps(12.4)).toBe(12);
  });
});
