import { describe, expect, it } from "vitest";

import { fileSource, locate } from "./reader";

describe("reader", () => {
  it("reads file sources and ignores web links", () => {
    expect(fileSource("file:///Users/me/My%20Lease.pdf#page=3")).toEqual({ path: "/Users/me/My Lease.pdf", page: 3 });
    expect(fileSource("file:///Users/me/notes.md")).toEqual({ path: "/Users/me/notes.md", page: null });
    expect(fileSource("https://example.com/a")).toBeNull();
  });

  it("finds the cited passage even when spacing differs", () => {
    const text = "[Page 1]\nIntro.\n\n[Page 2]\nPets are allowed\nwith a $300 deposit and written consent.";
    const [s, e] = locate(text, "Pets are allowed with a $300 deposit and written consent.")!;
    expect(text.slice(s, e)).toBe("Pets are allowed\nwith a $300 deposit and written consent.");
    // A snippet cut mid-word still finds its start.
    expect(locate(text, "Pets are allowed with a $300 depo")).not.toBeNull();
    // Nothing matches: the page marker instead.
    const [ps] = locate(text, "completely different words here", 2)!;
    expect(text.slice(ps, ps + 8)).toBe("[Page 2]");
    expect(locate(text, "nothing like this at all", null)).toBeNull();
  });
});
