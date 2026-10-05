import { describe, expect, it } from "vitest";

import { cpuName, graphicsLabel, hardwareNote, isPc, machine } from "./platform";
import type { SystemInfo } from "./types";

const base = { chip: "", freeDiskBytes: 0, osVersion: "", cpuCores: 8, chipInfo: {} as SystemInfo["chipInfo"], gpuBudgetBytes: 0 };
const GIB = 2 ** 30;
const mac = (ram: number, apple = true): SystemInfo => ({ ...base, totalRamBytes: ram * GIB, appleSilicon: apple, platform: "macos" });
const pc = (ram: number, gpus: SystemInfo["gpus"] = []): SystemInfo => ({ ...base, totalRamBytes: ram * GIB, appleSilicon: false, platform: "windows", gpus });
const card = (name: string, gb: number) => ({ name, vendor: "nvidia" as const, dedicatedBytes: gb * GIB, sharedBytes: 0, integrated: false });

describe("the first-launch hardware note", () => {
  it("says what it always said on a Mac", () => {
    expect(hardwareNote(mac(8))?.text).toBe("With 8 GB of memory, BYTE will use the smaller Fast model.");
    expect(hardwareNote(mac(16))?.text).toBe("Your Mac can run BYTE's Smart model comfortably.");
    expect(hardwareNote(mac(32))?.text).toBe("Your Mac has plenty of memory — every BYTE model will run well.");
    expect(hardwareNote(mac(16, false))?.ok).toBe(false);
  });
  it("never calls a PC unsupported just because it is not Apple Silicon", () => {
    // This was the bug: appleSilicon is false on every PC, so every one was warned off.
    for (const s of [pc(32, [card("NVIDIA GeForce RTX 4070", 12)]), pc(16), pc(8)]) {
      expect(hardwareNote(s)?.text).not.toMatch(/Apple Silicon|built for/);
    }
  });
  it("describes the card on a PC, by how much video memory it has", () => {
    expect(hardwareNote(pc(32, [card("NVIDIA GeForce RTX 4070", 12)]))?.text).toMatch(/12 GB of video memory, enough to run BYTE's larger models/);
    expect(hardwareNote(pc(16, [card("NVIDIA GeForce GTX 1060", 6)]))?.text).toMatch(/runs BYTE's Fast and Smart models well/);
    expect(hardwareNote(pc(16, [card("NVIDIA GeForce GTX 1650", 4)]))?.text).toMatch(/small for AI/);
  });
  it("is honest about a PC with no card, and flags only the very small ones", () => {
    expect(hardwareNote(pc(16))).toMatchObject({ ok: true, text: expect.stringMatching(/on your processor/) });
    expect(hardwareNote(pc(8))?.ok).toBe(false);
  });
});

describe("platform words", () => {
  it("names the machine from the backend, not the browser", () => {
    expect(machine(mac(16))).toBe("Mac");
    expect(machine(pc(16))).toBe("PC");
    expect(isPc({ platform: "linux" })).toBe(true);
  });
  it("tidies a processor name", () => {
    expect(cpuName("AMD Ryzen 7 7700X 8-Core Processor")).toBe("AMD Ryzen 7 7700X");
    expect(cpuName("Intel(R) Core(TM) i7-13700F")).toBe("Intel Core i7-13700F");
  });
  it("labels the graphics a PC will use", () => {
    expect(graphicsLabel(pc(32, [card("NVIDIA GeForce RTX 4070", 12)]))).toBe("NVIDIA GeForce RTX 4070 · 12 GB");
    expect(graphicsLabel(pc(16))).toBe("None found");
    expect(graphicsLabel(pc(16, [{ ...card("AMD Radeon(TM) Graphics", 0), integrated: true }]))).toBe("AMD Radeon(TM) Graphics (integrated)");
  });
});
