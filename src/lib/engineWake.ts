import type { EngineStatus } from "./types";

/**
 * Whether BYTE should start the model again now that the app is back on screen. Android can close the
 * engine (or the whole app) while BYTE is in the background; without this the user would see "No model"
 * and have to go to Settings. One try per 20 seconds, never while an answer or download is running.
 */
export function shouldWake(o: {
  engine: EngineStatus;
  answering: boolean;
  downloading: boolean;
  onCloud: boolean;
  hasDownloadedModel: boolean;
  now: number;
  lastWake: number;
}): boolean {
  if (o.engine.state === "ready" || o.engine.state === "starting") return false;
  if (o.answering || o.downloading || o.onCloud || !o.hasDownloadedModel) return false;
  return o.now - o.lastWake > 20_000;
}
