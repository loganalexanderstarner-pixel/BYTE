import { describe, expect, it } from "vitest";

import { clock, concat, downsample, encodeWav, level, toBase64 } from "./wav";

describe("voice recording", () => {
  it("downsamples by averaging", () => {
    const x = new Float32Array([0, 1, 0, 1, 0.5, 0.5]);
    const d = downsample(x, 48_000, 16_000);
    expect(d.length).toBe(2);
    expect(d[0]).toBeCloseTo(1 / 3, 5);
    expect(d[1]).toBeCloseTo(2 / 3, 5);
    expect(downsample(x, 16_000, 16_000)).toBe(x);
    expect(() => downsample(x, 8_000, 16_000)).toThrow();
  });

  it("writes a 16 kHz mono 16-bit WAV", () => {
    const w = encodeWav(new Float32Array([0, 1, -1, 2]));
    const v = new DataView(w.buffer);
    expect(String.fromCharCode(...w.subarray(0, 4))).toBe("RIFF");
    expect(String.fromCharCode(...w.subarray(8, 12))).toBe("WAVE");
    expect(w.length).toBe(44 + 8);
    expect(v.getUint32(24, true)).toBe(16_000);
    expect(v.getUint16(22, true)).toBe(1);
    expect([0, 1, 2, 3].map((i) => v.getInt16(44 + i * 2, true))).toEqual([0, 32767, -32768, 32767]);
  });

  it("encodes base64, levels and time", () => {
    expect(toBase64(new Uint8Array([72, 105]))).toBe("SGk=");
    expect(level(new Float32Array([]))).toBe(0);
    expect(level(new Float32Array([0.5, -0.5]))).toBe(1);
    expect([...concat([new Float32Array([1]), new Float32Array([2, 3])])]).toEqual([1, 2, 3]);
    expect(clock(7)).toBe("0:07");
    expect(clock(83.9)).toBe("1:23");
  });
});
