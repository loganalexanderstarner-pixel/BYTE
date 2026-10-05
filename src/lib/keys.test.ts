import { describe, expect, it } from "vitest";

import { keysFromEvent, keysProblem, platformKeys, prettyKeys } from "./keys";

const ev = (code: string, m: Partial<{ alt: boolean; ctrl: boolean; meta: boolean; shift: boolean }> = {}) => ({
  code,
  altKey: !!m.alt,
  ctrlKey: !!m.ctrl,
  metaKey: !!m.meta,
  shiftKey: !!m.shift,
});

describe("shortcut keys", () => {
  it("reads like a Mac menu", () => {
    expect(prettyKeys("Alt+Space", false)).toBe("⌥Space");
    expect(prettyKeys("Alt+Super+KeyB", false)).toBe("⌥⌘B");
    expect(prettyKeys("Super+Shift+Control+Digit1", false)).toBe("⌃⇧⌘1");
  });

  it("records a key press", () => {
    expect(keysFromEvent(ev("Space", { alt: true }))).toBe("Alt+Space");
    expect(keysFromEvent(ev("KeyB", { alt: true, meta: true }))).toBe("Alt+Super+KeyB");
    expect(keysFromEvent(ev("AltLeft", { alt: true }))).toBeNull();
    expect(keysFromEvent(ev("MetaRight", { meta: true }))).toBeNull();
  });

  it("refuses keys that would block typing", () => {
    expect(keysProblem("Alt+Space")).toBeNull();
    expect(keysProblem("Shift+KeyA", false)).toMatch(/⌘, ⌥ or ⌃/);
    expect(keysProblem("KeyA", false)).toMatch(/⌘, ⌥ or ⌃/);
  });
});

describe("shortcuts on Windows", () => {
  it("name their modifiers in Windows' order, with Super as the Windows key", () => {
    expect(prettyKeys("Control+Alt+KeyB", true)).toBe("Ctrl+Alt+B");
    expect(prettyKeys("Alt+Super+KeyB", true)).toBe("Alt+Win+B");
    expect(prettyKeys("Super+Shift+Control+Digit1", true)).toBe("Ctrl+Shift+Win+1");
    expect(prettyKeys("Alt+Space", true)).toBe("Alt+Space");
  });
  it("treat CmdOrCtrl as Ctrl there and as Command on a Mac", () => {
    expect(prettyKeys("CmdOrCtrl+Shift+KeyK", true)).toBe("Ctrl+Shift+K");
    expect(prettyKeys("CmdOrCtrl+Shift+KeyK", false)).toBe("⇧⌘K");
  });
  it("say which keys to add in Windows' words", () => {
    expect(keysProblem("KeyA", true)).toBe("Add Ctrl, Alt or Win, so normal typing still works.");
  });
});

describe("platformKeys", () => {
  it("is the identity on a Mac", () => {
    expect(platformKeys("New chat (⌘N)", false)).toBe("New chat (⌘N)");
  });
  it("writes Mac glyphs as Windows key names", () => {
    expect(platformKeys("New chat (⌘N)", true)).toBe("New chat (Ctrl+N)");
    expect(platformKeys("Enter / ⇧Enter", true)).toBe("Enter / Shift+Enter");
    expect(platformKeys("⌥Space", true)).toBe("Alt+Space");
  });
});
