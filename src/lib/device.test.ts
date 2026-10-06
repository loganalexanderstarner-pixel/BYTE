import { describe, expect, it } from "vitest";

import { deviceOf, ramLabel } from "./device";

const GIB = 2 ** 30;

describe("device wording", () => {
  it("names the device", () => {
    expect(deviceOf({ phone: true, appleSilicon: false })).toBe("phone");
    expect(deviceOf({ phone: false, appleSilicon: true })).toBe("Mac");
    expect(deviceOf({ phone: false, appleSilicon: false })).toBe("computer");
    // Before system info arrives the Mac app reads as it always did.
    expect(deviceOf(null)).toBe("Mac");
  });

  it("shows a phone's RAM as the size on the box", () => {
    // What the owner's Fold reported: 11 GiB of a 12 GB phone.
    expect(ramLabel({ phone: true, totalRamBytes: 11.1 * GIB })).toBe("12 GB");
    expect(ramLabel({ phone: true, totalRamBytes: 15.2 * GIB })).toBe("16 GB");
    expect(ramLabel({ phone: true, totalRamBytes: 5.4 * GIB })).toBe("6 GB");
    expect(ramLabel({ phone: false, totalRamBytes: 16 * GIB })).toBe("16 GB");
  });
});

import { onDevice } from "./device";

describe("onDevice", () => {
  it("rewrites Mac wording for a phone and leaves the Mac alone", () => {
    expect(onDevice("≈ 12 tokens/sec on this Mac", "phone")).toBe(
      "≈ 12 tokens/sec on this phone",
    );
    expect(onDevice("Everything runs on this Mac's GPU", "phone")).toBe(
      "Everything runs on this phone's GPU",
    );
    expect(onDevice("This Mac", "phone")).toBe("This phone");
    expect(onDevice("Runs entirely on your Mac", "computer")).toBe(
      "Runs entirely on your computer",
    );
    expect(onDevice("Runs entirely on your Mac", "Mac")).toBe(
      "Runs entirely on your Mac",
    );
  });
});

describe("onDevice adjectives", () => {
  it("rewrites 'smaller or older Mac' on a phone and leaves a Mac alone", async () => {
    const { onDevice } = await import("./device");
    expect(onDevice("Quick answers on a smaller or older Mac", "phone")).toBe("Quick answers on a smaller or older phone");
    expect(onDevice("Quick answers on a smaller or older Mac", "Mac")).toBe("Quick answers on a smaller or older Mac");
  });
});
