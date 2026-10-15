import { describe, expect, it, vi } from "vitest";

vi.mock("./api", () => ({ inTauri: false, api: {} }));

import { whoIsSpeaking } from "./voiceId";

describe("whoIsSpeaking", () => {
  it("sends nothing anywhere without BYTE Cloud or taught voices", async () => {
    expect(await whoIsSpeaking("AAAA", false)).toBeUndefined();
    expect(await whoIsSpeaking("AAAA", true)).toBeUndefined(); // not in the app: nobody taught
  });
});
