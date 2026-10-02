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


describe("toWire with files", () => {
  it("sends files read on this Mac with their user message only", () => {
    const file = { name: "notes.pdf", kind: "pdf" as const, pages: 3, text: "[Page 1]\nhello", truncated: false };
    const wire = toWire([{ ...msg("user", "summarise"), files: [file] }, { ...msg("assistant", "ok"), files: [file] }]);
    expect(wire).toEqual([
      { role: "user", content: "summarise", files: [file] },
      { role: "assistant", content: "ok" },
    ]);
  });
});
