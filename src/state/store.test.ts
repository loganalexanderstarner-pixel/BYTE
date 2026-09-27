import { describe, expect, it } from "vitest";

import { pickDefault } from "../components/onboarding/Onboarding";
import type { ModelStatus } from "../lib/types";
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

const model = (id: string, over: Partial<ModelStatus> = {}): ModelStatus => ({
  id,
  name: id,
  tagline: "",
  repo: "",
  file: "",
  sizeBytes: 1,
  role: "chat",
  recommended: false,
  thinking: true,
  speedHint: "",
  installed: false,
  partialBytes: 0,
  downloading: false,
  fit: { fit: "great", context: 16384, neededBytes: 1, gpuBudgetBytes: 1, totalRamBytes: 1, note: "" },
  ...over,
});

describe("pickDefault", () => {
  it("prefers the recommended model when it fits well", () => {
    expect(pickDefault([model("8b"), model("14b", { recommended: true })])?.id).toBe("14b");
  });

  it("falls back to a model that fits when the recommended one is too big", () => {
    const tooBig = { fit: "toobig" as const, context: 0, neededBytes: 1, gpuBudgetBytes: 1, totalRamBytes: 1, note: "" };
    expect(pickDefault([model("14b", { recommended: true, fit: tooBig }), model("8b")])?.id).toBe("8b");
  });

  it("never picks helper models", () => {
    expect(pickDefault([model("embed", { role: "embed" })])).toBeUndefined();
  });
});
