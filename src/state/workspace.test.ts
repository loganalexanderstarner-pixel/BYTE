import { describe, expect, it } from "vitest";

import type { Settings } from "../lib/types";
import { cloudChatsFrom, spaceOf, useStore, workspaceOf, type Conversation, type Message } from "./store";

const settings = (patch: Partial<Settings>) => ({ cloudConnected: true, workspace: "local", useCloud: false, ...patch }) as Settings;

describe("workspaces", () => {
  it("knows a chat's workspace from its id", () => {
    expect(spaceOf("cloud-42")).toBe("cloud");
    expect(spaceOf("both-7d1c")).toBe("both");
    expect(spaceOf("3f2a-local-uuid")).toBe("local");
  });

  it("uses Cloud and Both only while connected", () => {
    expect(workspaceOf(settings({ workspace: "both" }))).toBe("both");
    expect(workspaceOf(settings({ workspace: "cloud", cloudConnected: false }))).toBe("local");
    expect(workspaceOf(null)).toBe("local");
    // Settings saved before workspaces existed: the old cloud switch still counts.
    expect(workspaceOf(settings({ workspace: "local", useCloud: true }))).toBe("cloud");
  });

  it("reads the cloud's chat list in any shape, newest first", () => {
    const list = cloudChatsFrom({
      conversations: [
        { id: 1, title: "Old", updated_at: "2026-01-01T00:00:00Z" },
        { id: "2", name: "New", updatedAt: 1_790_000_000 },
        { title: "no id" },
      ],
    });
    expect(list.map((c) => [c.id, c.title])).toEqual([
      ["2", "New"],
      ["1", "Old"],
    ]);
    expect(cloudChatsFrom(null)).toEqual([]);
  });
});

describe("keeping one of two answers", () => {
  const answer = (id: string, extra: Partial<Message>): Message => ({ id, role: "assistant", content: id, status: "done", createdAt: 0, group: "g", ...extra });

  it("makes the chosen answer the one the chat continues from", () => {
    const conv: Conversation = {
      id: "both-1",
      title: "t",
      createdAt: 0,
      updatedAt: 0,
      loaded: true,
      messages: [{ id: "q", role: "user", content: "q", status: "done", createdAt: 0 }, answer("mac", {}), answer("cloud", { alt: true, cloud: true })],
    };
    useStore.setState({ conversations: [conv] });
    useStore.getState().keepAnswer("cloud");
    const msgs = useStore.getState().conversations[0].messages;
    expect(msgs.find((m) => m.id === "cloud")).toMatchObject({ alt: false, kept: true });
    expect(msgs.find((m) => m.id === "mac")).toMatchObject({ alt: true });
    useStore.getState().keepAnswer("mac");
    expect(useStore.getState().conversations[0].messages.find((m) => m.id === "cloud")?.alt).toBe(true);
  });
});
