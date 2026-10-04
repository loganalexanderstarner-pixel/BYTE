import { describe, expect, it } from "vitest";

import { cloudTurn, type Message } from "./store";

const m = (role: "user" | "assistant", remoteId?: string): Message => ({
  id: crypto.randomUUID(),
  role,
  content: "x",
  status: "done",
  remoteId,
  createdAt: 0,
});

describe("cloudTurn", () => {
  it("starts a cloud conversation for a new chat", () => {
    expect(cloudTurn({ messages: [m("user")], cloudId: null }, "auto")).toEqual({ conversationId: null, mode: "auto", lastRemoteId: null, branchFrom: null });
  });

  it("continues the cloud conversation after the last answer", () => {
    const messages = [m("user", "1"), m("assistant", "2"), m("user")];
    expect(cloudTurn({ messages, cloudId: "c", cloudHead: "2" }, "fast")).toEqual({ conversationId: "c", mode: "fast", lastRemoteId: "2", branchFrom: null });
  });

  it("forks after an edit (the cloud has moved on from where the chat now continues)", () => {
    // Edited second question: the chat continues from answer 2, but the cloud's head is answer 4.
    const messages = [m("user", "1"), m("assistant", "2"), m("user")];
    expect(cloudTurn({ messages, cloudId: "c", cloudHead: "4" }, "auto").branchFrom).toBe("2");
  });

  it("forks when the same question is asked again (regenerate)", () => {
    const messages = [m("user", "1"), m("assistant", "2"), m("user", "3")];
    expect(cloudTurn({ messages, cloudId: "c", cloudHead: "4" }, "auto")).toMatchObject({ conversationId: "c", branchFrom: "2" });
  });

  it("starts over when the very first message changed", () => {
    expect(cloudTurn({ messages: [m("user", "1")], cloudId: "c", cloudHead: "2" }, "auto").conversationId).toBeNull();
  });
});
