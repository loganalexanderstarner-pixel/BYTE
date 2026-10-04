import { describe, expect, it } from "vitest";

import { applyResult, changedWords, changes, cleanResult, fileName, target, toBase64 } from "./writing";

describe("changes", () => {
  it("marks only the new words and joins back into the result", () => {
    const p = changes("The quick brown fox jumps.", "The quick red fox leaps.");
    expect(p.map((x) => x.text).join("")).toBe("The quick red fox leaps.");
    expect(p.filter((x) => x.added).map((x) => x.text.trim())).toEqual(["red", "leaps."]);
    expect(changedWords(p)).toBe(2);
  });
  it("finds nothing new when only the case changed", () => {
    expect(changedWords(changes("hello World", "Hello world"))).toBe(0);
  });
  it("marks everything new when there was no original", () => {
    expect(changes("", "All new text")).toEqual([{ text: "All new text", added: true }]);
  });
});

describe("cleanResult", () => {
  it("removes what small models wrap the text in", () => {
    expect(cleanResult("Here is the rewritten text:\n\nHello there.")).toBe("Hello there.");
    expect(cleanResult("Sure! Here's a shorter version:\nHi.")).toBe("Hi.");
    expect(cleanResult("```\nHello.\n```")).toBe("Hello.");
    expect(cleanResult("<<<\nHello.\n>>>")).toBe("Hello.");
    expect(cleanResult('"Hello there."')).toBe("Hello there.");
  });
  it("keeps quotes that belong to the text", () => {
    expect(cleanResult('"Yes," she said. "Now."')).toBe('"Yes," she said. "Now."');
    expect(cleanResult("Here is where we met: the park.")).toBe("Here is where we met: the park.");
  });
});

describe("selections", () => {
  it("works on the selection, or on everything", () => {
    expect(target("abc def", 4, 7)).toEqual({ from: 4, to: 7 });
    // A selected space is not a selection.
    expect(target("abc def", 3, 4)).toEqual({ from: 0, to: 7 });
    expect(target("abc", 1, 1)).toEqual({ from: 0, to: 3 });
  });
  it("puts the result where the selection was, keeping its spacing", () => {
    expect(applyResult("One. Two bad sentence. Three.", 4, 23, "Two good sentences.")).toBe("One. Two good sentences. Three.");
    expect(applyResult("all", 0, 3, "  ALL  ")).toBe("ALL");
  });
});

describe("saving", () => {
  it("makes safe file names and UTF-8 base64", () => {
    expect(fileName("Why Sleep Matters: A Guide?")).toBe("Why Sleep Matters A Guide.md");
    expect(fileName("  ")).toBe("Writing.md");
    expect(atob(toBase64("café"))).toBe("caf\u00c3\u00a9");
  });
});
