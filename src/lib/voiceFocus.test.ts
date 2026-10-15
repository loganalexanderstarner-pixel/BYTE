import { describe, expect, it } from "vitest";

import { isTaught, parseFocusCommand } from "./voiceFocus";

describe("parseFocusCommand", () => {
  it("hears listen to everyone", () => {
    for (const t of ["listen to everyone", "Listen to everybody.", "Hey BYTE, listen to everyone", "please listen to everyone now"]) expect(parseFocusCommand(t), t).toEqual({ kind: "everyone" });
  });
  it("hears stop focusing on me", () => {
    for (const t of ["stop focusing on me", "Stop focusing on me.", "stop focusing", "don't focus on me"]) expect(parseFocusCommand(t), t).toEqual({ kind: "stopFocusing" });
  });
  it("hears also listen to a name", () => {
    expect(parseFocusCommand("also listen to Anna")).toEqual({ kind: "alsoListenTo", name: "Anna" });
    expect(parseFocusCommand("Also listen to anna, too.")).toEqual({ kind: "alsoListenTo", name: "Anna" });
    expect(parseFocusCommand("and listen to bob as well")).toEqual({ kind: "alsoListenTo", name: "Bob" });
  });
  it("leaves ordinary sentences alone", () => {
    for (const t of ["what's the weather", "listen to this song", "also what time is it", "also listen to everyone", "also listen to this song", "and hear the radio", "also listen to mary jane", "play music", "stop"]) expect(parseFocusCommand(t), t).toBeNull();
  });
});

describe("isTaught", () => {
  it("matches a taught name regardless of case", () => {
    expect(isTaught("anna", ["Anna", "Bob"])).toBe(true);
    expect(isTaught("Carl", ["Anna", "Bob"])).toBe(false);
  });
});
