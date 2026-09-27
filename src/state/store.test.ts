import { describe, expect, it } from "vitest";

import { toWire, type Message } from "./store";

const msg = (role: Message["role"], content: string, status: Message["status"] = "done"): Message => ({
  id: Math.random().toString(36),
  role,
  content,
  status,
  createdAt: 0,
});

describe("toWire", () => {
  it("sends finished turns and drops failed or empty replies", () => {
    const wire = toWire([
      msg("user", "hi"),
      msg("assistant", "hello"),
      msg("user", "again"),
      msg("assistant", "", "error"),
      msg("assistant", "partial", "cancelled"),
      msg("user", "third"),
    ]);
    expect(wire).toEqual([
      { role: "user", content: "hi" },
      { role: "assistant", content: "hello" },
      { role: "user", content: "again" },
      { role: "user", content: "third" },
    ]);
  });
});

