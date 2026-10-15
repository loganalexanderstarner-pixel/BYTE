import { describe, expect, it } from "vitest";

import { endsUnfinished, joinTurns, TurnJoiner } from "./turnJoin";

describe("endsUnfinished", () => {
  it("spots turns that stop mid-thought", () => {
    for (const t of ["and then", "set a timer for", "I want the", "um", "remind me to uh", "what is the weather and", "play something but", "Okay so,"]) expect(endsUnfinished(t), t).toBe(true);
  });
  it("leaves finished turns alone", () => {
    for (const t of ["what time is it", "set a timer for ten minutes", "turn it down", "what is it?", "thanks.", "turn the lights on", "what is that", "I live in", "it is", "play some music", "", "yes"]) expect(endsUnfinished(t), t).toBe(false);
  });
});

describe("joinTurns", () => {
  it("appends the continuation and drops fillers", () => {
    expect(joinTurns("set a timer for um", "ten minutes")).toBe("set a timer for ten minutes");
    expect(joinTurns("and then,", "turn off the lights")).toBe("and then turn off the lights");
    expect(joinTurns("remind me to", "")).toBe("remind me to");
    expect(joinTurns("um", "what time is it")).toBe("what time is it");
  });
});

describe("TurnJoiner", () => {
  it("sends a finished turn at once", () => {
    const j = new TurnJoiner();
    expect(j.firstText("what time is it", true)).toEqual({ kind: "send", text: "what time is it" });
  });
  it("sends an unfinished turn as it is when the setting is off", () => {
    expect(new TurnJoiner().firstText("and then", false)).toEqual({ kind: "send", text: "and then" });
  });
  it("waits, then appends what was said next to the same turn", () => {
    const j = new TurnJoiner();
    expect(j.firstText("set a timer for", true)).toEqual({ kind: "wait" });
    expect(j.moreArrived("WAV")).toEqual({ kind: "transcribe", wav: "WAV" });
    expect(j.moreText("ten minutes")).toEqual({ kind: "send", text: "set a timer for ten minutes" });
  });
  it("sends the first part when nothing more is said", () => {
    const j = new TurnJoiner();
    j.firstText("play some music and", true);
    expect(j.moreArrived(null)).toEqual({ kind: "send", text: "play some music and" });
  });
  it("copes with the extra audio arriving before the text", () => {
    const j = new TurnJoiner();
    expect(j.moreArrived("WAV")).toEqual({ kind: "wait" });
    expect(j.firstText("what is the", true)).toEqual({ kind: "transcribe", wav: "WAV" });
    j.reset();
    expect(j.moreArrived(null)).toEqual({ kind: "wait" });
    expect(j.firstText("and then", true)).toEqual({ kind: "send", text: "and then" });
  });
  it("gives up on request and uses the extra second only once", () => {
    const j = new TurnJoiner();
    j.firstText("so", true);
    expect(j.giveUp()).toEqual({ kind: "send", text: "so" });
    expect(j.giveUp()).toEqual({ kind: "wait" });
  });
});
