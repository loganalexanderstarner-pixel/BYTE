import { describe, expect, it } from "vitest";

import { engineAfterLoad } from "./engineState";
import type { EngineStatus } from "./types";

const stopped = { state: "stopped" } as unknown as EngineStatus;
const ready = { state: "ready" } as unknown as EngineStatus;

describe("engineAfterLoad", () => {
  it("takes the fetched state when nothing changed meanwhile", () => {
    expect(engineAfterLoad(stopped, ready, 0)).toBe(ready);
  });
  it("keeps what an event already said when the fetched state is older", () => {
    expect(engineAfterLoad(ready, stopped, 1)).toBe(ready);
    expect(engineAfterLoad(ready, stopped, 3)).toBe(ready);
  });
});
