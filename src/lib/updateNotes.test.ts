import { describe, expect, it } from "vitest";

import { updateSummary } from "./updateNotes";

describe("updateSummary", () => {
  it("joins the first paragraph and drops Markdown", () => {
    const notes = "# Title (v0.12.4)\n\n**BYTE v0.12.4** is the first version that installs itself: open Settings → About,\nclick **Check for updates**. See [docs/VERSIONS.md](https://example.com).\n\n## New\n- x";
    expect(updateSummary(notes)).toBe("BYTE v0.12.4 is the first version that installs itself: open Settings → About, click Check for updates. See docs/VERSIONS.md.");
  });
  it("is empty without a paragraph", () => {
    expect(updateSummary("# Only a title")).toBe("");
  });
});
