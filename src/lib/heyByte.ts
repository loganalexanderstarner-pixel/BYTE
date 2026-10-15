/** Where a spoken question ("Hey BYTE", Talk mode) is answered: the model on this PC or BYTE Cloud. */
export type HeyEngine = "local" | "cloud" | "auto";

export const HEY_ENGINES: { id: HeyEngine; label: string }[] = [
  { id: "auto", label: "Automatic" },
  { id: "local", label: "This PC" },
  { id: "cloud", label: "BYTE Cloud" },
];

/**
 * True when the spoken question should go to the cloud.
 * - No connected cloud account: never.
 * - "cloud" / "local": as chosen.
 * - "auto": the cloud while the PC is busy (a game is running full screen, or the graphics card is working hard) or when no
 *   model is loaded on this PC; this PC otherwise. `busy` is null when it could not be read: then this PC.
 */
export function heyByteUsesCloud(engine: string | undefined, cloudConnected: boolean, busy: boolean | null, localReady: boolean): boolean {
  if (!cloudConnected) return false;
  if (engine === "cloud") return true;
  if (engine === "local") return false;
  return busy === true || !localReady;
}
