import { describe, expect, it } from "vitest";

import { bytes, contextLabel, duration, eta, ramSize, titleFrom, tokensPerSec } from "./format";

describe("format", () => {
  it("formats byte sizes in decimal units", () => {
    expect(bytes(9_001_752_960)).toBe("9.0 GB");
    expect(bytes(639_446_688)).toBe("639.4 MB");
    expect(bytes(0)).toBe("0 B");
    expect(bytes(512)).toBe("512 B");
  });

  it("rounds RAM to marketing sizes", () => {
    expect(ramSize(17_179_869_184)).toBe("16 GB");
    expect(ramSize(25_769_803_776)).toBe("24 GB");
  });

  it("formats durations and ETAs", () => {
    expect(duration(12.4)).toBe("12 s");
    expect(duration(95)).toBe("1 min 35 s");
    expect(duration(3700)).toBe("1 h 1 min");
    expect(eta(0, 100, 10)).toBe("10 s");
    expect(eta(100, 100, 10)).toBe("—");
    expect(eta(0, 100, 0)).toBe("—");
  });

  it("formats speeds and context", () => {
    expect(tokensPerSec(12.345)).toBe("12.3 tok/s");
    expect(tokensPerSec(150.4)).toBe("150 tok/s");
    expect(contextLabel(16384)).toBe("16k");
  });

  it("builds short titles on word boundaries", () => {
    expect(titleFrom("  hello   world ")).toBe("hello world");
    expect(titleFrom("")).toBe("New chat");
    const t = titleFrom("Explain how compound interest works with a simple example please", 30);
    expect(t.endsWith("…")).toBe(true);
    expect(t.length).toBeLessThanOrEqual(31);
    expect(t).not.toMatch(/\s…$/);
  });
});
