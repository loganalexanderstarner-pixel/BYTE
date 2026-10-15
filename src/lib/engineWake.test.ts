import { describe, expect, it } from "vitest";
import { shouldWake } from "./engineWake";

const base = { answering: false, downloading: false, onCloud: false, hasDownloadedModel: true, now: 100_000, lastWake: 0 };

describe("shouldWake", () => {
  it("restarts a stopped, missing or failed engine when a model is downloaded", () => {
    for (const state of ["stopped", "noModel", "error"] as const) {
      const engine = state === "error" ? { state, message: "x" } : { state };
      expect(shouldWake({ ...base, engine: engine as never })).toBe(true);
    }
  });
  it("leaves a running or loading engine alone", () => {
    expect(shouldWake({ ...base, engine: { state: "ready", model: "m", context: 8192, boosted: false, vision: false } })).toBe(false);
    expect(shouldWake({ ...base, engine: { state: "starting", model: "m" } })).toBe(false);
  });
  it("does nothing without a downloaded model, on the cloud, or while busy", () => {
    const engine = { state: "stopped" } as const;
    expect(shouldWake({ ...base, engine, hasDownloadedModel: false })).toBe(false);
    expect(shouldWake({ ...base, engine, onCloud: true })).toBe(false);
    expect(shouldWake({ ...base, engine, answering: true })).toBe(false);
    expect(shouldWake({ ...base, engine, downloading: true })).toBe(false);
  });
  it("tries once per 20 seconds", () => {
    const engine = { state: "stopped" } as const;
    expect(shouldWake({ ...base, engine, lastWake: 90_000 })).toBe(false);
    expect(shouldWake({ ...base, engine, lastWake: 70_000 })).toBe(true);
  });
});
