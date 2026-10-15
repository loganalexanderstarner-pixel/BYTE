import { describe, expect, it } from "vitest";

import { transcript, when } from "./messages";

describe("messages", () => {
  it("writes the conversation for the reply drafter", () => {
    const msgs = [
      { text: "Dinner at 6?", at: 1, fromMe: false, sender: "Mom" },
      { text: "Yes!", at: 2, fromMe: true, sender: "You" },
      { text: "Bring dessert", at: 3, fromMe: false, sender: "Mom" },
    ];
    expect(transcript(msgs)).toBe("Mom: Dinner at 6?\nMe: Yes!\nMom: Bring dessert");
    expect(transcript(msgs, "say I'll bring pie", 1)).toBe("Mom: Bring dessert\n\n(What I want to say in my reply: say I'll bring pie)");
  });
  it("shows a short time", () => {
    const now = new Date(2026, 9, 3, 15, 0).getTime();
    expect(when(new Date(2026, 9, 3, 14, 5).getTime(), now)).toMatch(/2:05/);
    expect(when(new Date(2026, 8, 1).getTime(), now)).toMatch(/Sep/);
  });
});
