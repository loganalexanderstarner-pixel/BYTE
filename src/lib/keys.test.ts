import { describe, expect, it } from "vitest";

import { keysFromEvent, keysProblem, prettyKeys } from "./keys";

const ev = (code: string, m: Partial<{ alt: boolean; ctrl: boolean; meta: boolean; shift: boolean }> = {}) => ({
  code,
  altKey: !!m.alt,
  ctrlKey: !!m.ctrl,
  metaKey: !!m.meta,
  shiftKey: !!m.shift,
});

describe("shortcut keys", () => {
  it("reads like a Mac menu", () => {
    expect(prettyKeys("Alt+Space")).toBe("⌥Space");
    expect(prettyKeys("Alt+Super+KeyB")).toBe("⌥⌘B");
    expect(prettyKeys("Super+Shift+Control+Digit1")).toBe("⌃⇧⌘1");
  });

  it("records a key press", () => {
    expect(keysFromEvent(ev("Space", { alt: true }))).toBe("Alt+Space");
    expect(keysFromEvent(ev("KeyB", { alt: true, meta: true }))).toBe("Alt+Super+KeyB");
    expect(keysFromEvent(ev("AltLeft", { alt: true }))).toBeNull();
    expect(keysFromEvent(ev("MetaRight", { meta: true }))).toBeNull();
  });

  it("refuses keys that would block typing", () => {
    expect(keysProblem("Alt+Space")).toBeNull();
    expect(keysProblem("Shift+KeyA")).toMatch(/⌘, ⌥ or ⌃/);
    expect(keysProblem("KeyA")).toMatch(/⌘, ⌥ or ⌃/);
  });
});
