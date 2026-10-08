import { describe, expect, it } from "vitest";

import { articlesFor } from "./help";
import { forPlatform, localize } from "./helpText";

const files = import.meta.glob("../help/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

describe("platform blocks", () => {
  const text = ["before", "<!-- mac -->", "mac only", "<!-- win -->", "windows only", "<!-- all -->", "after"].join("\n");
  it("keeps the Mac block on a Mac and the Windows block on Windows", () => {
    expect(forPlatform(text, false)).toBe("before\nmac only\nafter");
    expect(forPlatform(text, true)).toBe("before\nwindows only\nafter");
  });
  it("treats a block with no end as running to the end of the file", () => {
    expect(forPlatform("a\n<!-- mac -->\nb\nc", false)).toBe("a\nb\nc");
    expect(forPlatform("a\n<!-- mac -->\nb\nc", true)).toBe("a");
  });
  it("leaves text without markers alone on a Mac, and rewords it on Windows", () => {
    expect(localize("Press ⌘K on your Mac.", false)).toBe("Press ⌘K on your Mac.");
    expect(localize("Press ⌘K on your Mac.", true)).toBe("Press Ctrl+K on your PC.");
  });
});

describe("the bundled help articles", () => {
  const mac = articlesFor(false, files);
  const win = articlesFor(true, files);

  it("show no marker on either platform", () => {
    for (const a of [...mac, ...win]) expect(a.body, a.id).not.toMatch(/<!--/);
  });

  it("have the same articles on both platforms", () => {
    expect(win.map((a) => a.id)).toEqual(mac.map((a) => a.id));
  });

  it("never send a Windows reader to something only a Mac has", () => {
    // Words that mean a Mac to the person reading. "Mac" alone is allowed only where the article
    // says a feature is a Mac one (the Mac-features line in the hotkeys article).
    const mac_only = [/⌘/, /⌥/, /macOS/, /Finder/, /Touch ID/, /Keychain/, /iCloud/, /Open Anyway/, /System Settings/, /menu bar/i, /\bSiri\b/, /Gatekeeper/];
    for (const a of win) {
      for (const re of mac_only) expect(`${a.title}\n${a.body}`, `${a.id} mentions ${re}`).not.toMatch(re);
    }
  });

  it("word a Windows reader's own steps for Windows", () => {
    const get = (id: string) => win.find((a) => a.id === id)!.body;
    expect(get("getting-started")).toMatch(/Ctrl\+Alt\+Space/);
    expect(get("getting-started")).toMatch(/Run anyway/);
    expect(get("troubleshooting")).toMatch(/Run anyway/);
    expect(get("shortcuts")).toMatch(/\| Ctrl\+Alt\+B \|/);
    expect(win.find((a) => a.id === "mac-control")!.title).toBe("PC control, hotkeys and clipboard");
  });

  it("are unchanged on a Mac", () => {
    const get = (id: string) => mac.find((a) => a.id === id)!;
    expect(get("mac-control").title).toBe("Mac control and permissions");
    expect(get("getting-started").body).toMatch(/\*\*⌘K\*\*/);
    expect(get("troubleshooting").body).toMatch(/Open Anyway/);
  });
});
