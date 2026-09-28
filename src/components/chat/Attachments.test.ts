import { describe, expect, it } from "vitest";

import { fileDetail } from "./Attachments";

describe("fileDetail", () => {
  it("names pages, slides and sheets, and says when only the best parts are used", () => {
    expect(fileDetail({ name: "a.pdf", kind: "pdf", pages: 12, text: "x", truncated: false })).toBe("12 pages");
    expect(fileDetail({ name: "a.pptx", kind: "slides", pages: 1, text: "x", truncated: false })).toBe("1 slide");
    expect(fileDetail({ name: "a.xlsx", kind: "sheet", pages: 2, text: "x", truncated: true })).toBe("2 sheets · long: best parts used");
    expect(fileDetail({ name: "a.txt", kind: "text", text: "x", truncated: false })).toBe("");
  });
});
