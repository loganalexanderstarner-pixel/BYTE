import { describe, expect, it } from "vitest";

import { intervalText, missedAsCards, nextInterval, quizScore } from "./study";

describe("study helpers", () => {
  it("previews SM-2 intervals like the Rust side", () => {
    const fresh = { ease: 2.5, interval: 0, reps: 0 };
    expect(nextInterval(fresh, 4)).toBe(1);
    expect(nextInterval({ ease: 2.5, interval: 1, reps: 1 }, 4)).toBe(6);
    expect(nextInterval({ ease: 2.5, interval: 6, reps: 2 }, 4)).toBe(15);
    expect(nextInterval({ ease: 2.5, interval: 6, reps: 2 }, 5)).toBe(20);
    expect(nextInterval({ ease: 2.5, interval: 6, reps: 2 }, 3)).toBe(7);
    expect(nextInterval({ ease: 2.5, interval: 60, reps: 5 }, 1)).toBe(1);
  });

  it("describes intervals", () => {
    expect(intervalText(1)).toBe("tomorrow");
    expect(intervalText(6)).toBe("6 days");
    expect(intervalText(21)).toBe("3 wk");
    expect(intervalText(120)).toBe("4 mo");
    expect(intervalText(500)).toBe("1.4 yr");
  });

  it("scores quizzes and turns misses into cards", () => {
    const qs = [
      { question: "Gold?", choices: ["Ag", "Au"], answer: 1, explanation: "Latin aurum." },
      { question: "Water?", choices: ["H2O", "CO2"], answer: 0, explanation: "" },
    ];
    expect(quizScore([1, 1], [1, 0])).toEqual({ right: 1, total: 2, percent: 50 });
    expect(missedAsCards(qs, [1, 1])).toEqual([{ front: "Water?", back: "H2O" }]);
    expect(missedAsCards(qs, [0, null])).toEqual([{ front: "Gold?", back: "Au — Latin aurum." }]);
  });
});
