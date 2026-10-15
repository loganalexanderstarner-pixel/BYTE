import { afterEach, describe, expect, it, vi } from "vitest";

import { defaultQuickAskKeys, defaultSelectionKeys, isLinux, isPcOs, isWindows, keysFromEvent, keysProblem, platformKeys, prettyKeys } from "./keys";

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

describe("default shortcuts", () => {
  it("keep clear of what Windows already uses", () => {
    // Alt+Space is every Windows app's window menu; Alt+Win+B toggles HDR.
    expect(defaultQuickAskKeys(true)).not.toBe("Alt+Space");
    expect(defaultSelectionKeys(true)).not.toContain("Super");
    expect(prettyKeys(defaultQuickAskKeys(true), true)).toBe("Ctrl+Alt+Space");
    expect(prettyKeys(defaultSelectionKeys(true), true)).toBe("Ctrl+Alt+B");
  });
  it("are unchanged on a Mac", () => {
    expect(defaultQuickAskKeys(false)).toBe("Alt+Space");
    expect(defaultSelectionKeys(false)).toBe("Alt+Super+KeyB");
  });
});

describe("which computer this is", () => {
  afterEach(() => vi.unstubAllGlobals());
  const browser = (platform: string, userAgent: string) => vi.stubGlobal("navigator", { platform, userAgent });

  it("is Linux in the Linux app's web view", () => {
    browser("Linux x86_64", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15");
    expect(isLinux()).toBe(true);
    expect(isWindows()).toBe(false);
    expect(isPcOs()).toBe(true);
  });
  it("is not Linux in Node (the tests), which also reports platform linux", () => {
    browser("linux", "Node.js/22");
    expect(isLinux()).toBe(false);
    expect(isPcOs()).toBe(false);
  });
  it("is not Linux on Android, whose web view says Linux too", () => {
    browser("Linux armv8l", "Mozilla/5.0 (Linux; Android 15) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36");
    expect(isLinux()).toBe(false);
  });
  it("is Windows in the Windows app and a Mac otherwise", () => {
    browser("Win32", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/120");
    expect(isWindows()).toBe(true);
    expect(isPcOs()).toBe(true);
    browser("MacIntel", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15");
    expect(isPcOs()).toBe(false);
  });
  it("shows Linux readers PC keys", () => {
    browser("Linux x86_64", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15");
    expect(platformKeys("New chat (⌘N)")).toBe("New chat (Ctrl+N)");
    expect(defaultQuickAskKeys()).toBe("Control+Alt+Space");
  });
});
