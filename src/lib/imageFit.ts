import { bytes } from "./format";
import type { ImagePlacement, ImageVariant } from "./types";

/** One plain sentence about where a version would run on this PC, for the model card. */
export function fitSentence(v: ImageVariant): string {
  const f = v.fit;
  switch (f.placement) {
    case "vram":
      return `Fits in your graphics card's memory (needs about ${bytes(f.needBytes)}). Fastest.`;
    case "split":
      return `Does not fit in video memory by about ${bytes(f.overflowBytes)}. It will overflow into your RAM: it works, but pictures take longer.`;
    case "ram":
      return `No graphics card memory to use, so it runs on the processor from RAM (about ${bytes(f.needBytes)}). Works, but slowly.`;
    case "toobig":
      return `Too big: it needs about ${bytes(f.needBytes)} and this PC is ${bytes(f.overflowBytes)} short even with RAM. Pick a smaller version.`;
  }
}

export const PLACEMENT_LABEL: Record<ImagePlacement, string> = {
  vram: "Fits video memory",
  split: "Overflows into RAM",
  ram: "Runs from RAM",
  toobig: "Too big",
};

/** The pill class for a placement. */
export function placementClass(p: ImagePlacement): string {
  return p === "vram" ? "ok" : p === "toobig" ? "danger" : "warn";
}
