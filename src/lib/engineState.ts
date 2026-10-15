import type { EngineStatus } from "./types";

/**
 * The engine's state after the first load. The window asks for the current state while it starts
 * and also listens for changes; if a change arrives while the answer is on its way, the answer is
 * older than what the window already knows and must not overwrite it. (A small model on a fast
 * machine is ready within a second of launch, which is enough to hit this: the window then said
 * "Engine stopped" next to a working engine.)
 */
export function engineAfterLoad(current: EngineStatus, fetched: EngineStatus, eventsSinceAsking: number): EngineStatus {
  return eventsSinceAsking > 0 ? current : fetched;
}
