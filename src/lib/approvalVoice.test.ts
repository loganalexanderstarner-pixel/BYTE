import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(true)) }));

import { invoke } from "@tauri-apps/api/core";
import { approvalVoice, parseYesNo } from "./approvalVoice";

describe("spoken yes and no", () => {
  it("understands short clear answers", () => {
    for (const t of ["Yes.", "yeah", "Yes please", "go ahead", "Do it!", "Okay", "hey byte yes"]) expect(parseYesNo(t), t).toBe("yes");
    for (const t of ["No", "Nope.", "don't", "Do not do it", "cancel", "stop", "no thanks"]) expect(parseYesNo(t), t).toBe(t === "no thanks" ? "no" : "no");
  });
  it("treats anything else as unclear, never as yes", () => {
    for (const t of ["", "maybe", "what are you going to open", "yes and no", "I said yes to my mum earlier today", "um"]) expect(parseYesNo(t), t).toBe("unclear");
  });
  it("answers the approval on a clear word, asks once more, then leaves it to the buttons", () => {
    approvalVoice.begin("x1");
    expect(approvalVoice.hear("maybe")).toEqual({ say: "Please say yes or no.", answered: false, listenAgain: true });
    expect(invoke).not.toHaveBeenCalled();
    const r = approvalVoice.hear("yes");
    expect(r.answered).toBe(true);
    expect(invoke).toHaveBeenCalledWith("agent_approve", { id: "x1", ok: true, edits: null });
    expect(approvalVoice.pending()).toBeNull();
    approvalVoice.begin("x2");
    approvalVoice.hear("hmm");
    expect(approvalVoice.hear("hmm").listenAgain).toBe(false);
    expect(approvalVoice.pending()).toBeNull();
    expect(approvalVoice.hear("yes").answered).toBe(false);
  });
});
