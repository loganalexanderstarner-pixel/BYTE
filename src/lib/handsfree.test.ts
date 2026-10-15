import { describe, expect, it } from "vitest";

import { isDone, isStopPhrase, SilenceDetector } from "./handsfree";

const run = (levels: number[], step = 100) => {
  const d = new SilenceDetector(0);
  return levels.map((l, i) => d.push(l, (i + 1) * step));
};

describe("hands-free listening", () => {
  it("stops after a pause that follows speech", () => {
    const quiet = Array(5).fill(0.01);
    const talk = Array(10).fill(0.4);
    const out = run([...quiet, ...talk, ...Array(12).fill(0.01)]);
    expect(out.slice(0, 5).every((h) => h === "waiting")).toBe(true);
    expect(out[10]).toBe("speech");
    // One second of speech is a short utterance: the turn ends after 0.8 s of quiet, not 1.2 s.
    expect(out[20]).toBe("speech");
    expect(out[22]).toBe("done");
  });

  it("ends a short command sooner than a long sentence", () => {
    const step = 100;
    const doneAfter = (talkSteps: number) => {
      const d = new SilenceDetector(0, 1100);
      let t = 0;
      for (let i = 0; i < talkSteps; i++) d.push(0.4, (t += step));
      let quiet = 0;
      while (d.push(0.01, (t += step)) !== "done") quiet += step;
      return quiet + step;
    };
    expect(doneAfter(10)).toBeLessThanOrEqual(900); // one second of speech
    expect(doneAfter(30)).toBeGreaterThanOrEqual(1100); // three seconds of speech
  });

  it("gives up when nothing is said, and ignores a click", () => {
    const d = new SilenceDetector(0, 1200, 3000);
    expect(d.push(0.5, 100)).toBe("speech"); // a click: too short to count as speech
    expect(d.push(0.01, 200)).toBe("speech");
    expect(d.push(0.01, 3100)).toBe("nothing");
  });

  it("knows when the user is done talking", () => {
    for (const yes of ["Stop.", "That's all", "okay stop listening", "Goodbye!", "Thanks Byte, that's all", "never mind", "bye byte"]) expect(isStopPhrase(yes), yes).toBe(true);
    for (const no of ["Stop the timer at 5", "That's all the info I need about Rome?", "What's a goodbye letter?", "bye week schedule"]) expect(isStopPhrase(no), no).toBe(false);
  });

  it("ends a Hey BYTE follow-up on thanks", () => {
    for (const yes of ["Thanks!", "thank you byte", "No thanks", "That's it", "okay thanks", "Stop"]) expect(isDone(yes), yes).toBe(true);
    for (const no of ["Thanks, and what about tomorrow?", "thank you notes ideas", "and in Paris?"]) expect(isDone(no), no).toBe(false);
  });
});
