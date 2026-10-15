import { describe, expect, it } from "vitest";

import { toWire, type Message } from "../../state/store";
import { groupAnswers } from "./ChatView";

const msg = (id: string, role: Message["role"], extra: Partial<Message> = {}): Message => ({
  id,
  role,
  content: id,
  status: "done",
  createdAt: 0,
  ...extra,
});

describe("side-by-side answers", () => {
  const messages = [
    msg("q1", "user"),
    msg("a1", "assistant", { group: "g", model: "main:Q4_K_M" }),
    msg("a2", "assistant", { group: "g", model: "other:Q4_K_M", alt: true }),
    msg("q2", "user"),
    msg("a3", "assistant"),
  ];

  it("groups answers to the same question into one row", () => {
    const items = groupAnswers(messages);
    expect(items).toHaveLength(4);
    expect(Array.isArray(items[1]) && items[1].map((m) => m.id)).toEqual(["a1", "a2"]);
  });

  it("keeps only the main model's answer in the history", () => {
    expect(toWire(messages).map((m) => m.content)).toEqual(["q1", "a1", "q2", "a3"]);
  });
});
