import type { SystemInfo } from "./types";

/**
 * What to call the device BYTE is running on, in the words people use for it.
 * The UI was written for the Mac; on a phone it must not say "your Mac"
 * (docs/ANDROID.md). Mac-only features keep their Mac wording, since they are
 * hidden or replaced elsewhere.
 */
export type Device = "phone" | "Mac" | "computer";

export function deviceOf(
  system: Pick<SystemInfo, "phone" | "appleSilicon"> | null | undefined,
): Device {
  if (system?.phone) return "phone";
  if (!system || system.appleSilicon) return "Mac";
  return "computer";
}

/** Phone sizes people know: Android reports a little less than the label (11 GB on a 12 GB phone). */
const PHONE_SIZES = [3, 4, 6, 8, 12, 16, 18, 24, 32];

/** RAM as the box says it: "12 GB" on a 12 GB phone, the rounded size elsewhere. */
export function ramLabel(
  system: Pick<SystemInfo, "phone" | "totalRamBytes">,
): string {
  const gb = system.totalRamBytes / 2 ** 30;
  if (system.phone) {
    const size = PHONE_SIZES.find((s) => gb <= s) ?? Math.ceil(gb);
    return `${size} GB`;
  }
  return `${Math.round(gb)} GB`;
}

/**
 * Rewrites "this Mac" / "your Mac" / "This Mac" for the device BYTE runs on, so
 * shared screens read right on a phone. Mac-only feature text isn't passed here.
 */
export function onDevice(
  text: string,
  device: Device = currentDevice(),
): string {
  if (device === "Mac") return text;
  return text
    .replace(/\bthis Mac's\b/g, `this ${device}'s`)
    .replace(/\byour Mac's\b/g, `your ${device}'s`)
    .replace(/\bthis Mac\b/g, `this ${device}`)
    .replace(/\byour Mac\b/g, `your ${device}`)
    .replace(/\bThis Mac\b/g, `This ${device}`)
    .replace(/\bYour Mac\b/g, `Your ${device}`)
    .replace(/\b(older|smaller|slower|newer|any|a) Mac\b/g, `$1 ${device}`);
}

let known: Device = "Mac";
/** Set once system info arrives (store.ts), read by `onDevice`. */
export function setDevice(d: Device): void {
  known = d;
}
export function currentDevice(): Device {
  return known;
}
