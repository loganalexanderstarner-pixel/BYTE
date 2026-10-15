import { describe, expect, it } from "vitest";

import type { Conversation } from "../state/store";
import { listSignature } from "./throttle";

const conv = (patch: Partial<Conversation> = {}): Conversation => ({ id: "a", title: "Chat", createdAt: 0, updatedAt: 60_000, messages: [], messageCount: 2, ...patch });

describe("sidebar signature", () => {
  it("ignores streamed words but notices changes the sidebar shows", () => {
    const base = listSignature([conv()]);
    const streaming = conv({ messages: [{ id: "m", role: "assistant", content: "more and more text", status: "streaming", createdAt: 0 }], updatedAt: 60_500 });
    expect(listSignature([streaming])).toBe(base);
    expect(listSignature([conv({ title: "Renamed" })])).not.toBe(base);
    expect(listSignature([conv({ pinned: true })])).not.toBe(base);
    expect(listSignature([conv({ updatedAt: 200_000 })])).not.toBe(base);
    expect(listSignature([conv(), conv({ id: "b" })])).not.toBe(base);
  });
});
