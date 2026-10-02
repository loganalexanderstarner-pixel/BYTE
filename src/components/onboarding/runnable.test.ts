import { describe, expect, it } from "vitest";

import type { ModelStatus } from "../../lib/types";
import { runnable } from "./Onboarding";

const model = (id: string, bits: number, quality: number, size: number) =>
  ({
    id,
    role: "chat",
    best: `${id}:q`,
    variants: [{ key: `${id}:q`, bits, quality, sizeBytes: size }],
  }) as unknown as ModelStatus;

describe("runnable", () => {
  it("lists good-quality versions before squeezed ones", () => {
    const list = runnable([model("big", 2.5, 95, 8e9), model("mid", 6.5, 80, 7e9), model("small", 4.5, 70, 3e9)]);
    expect(list.map((x) => x.model.id)).toEqual(["mid", "small", "big"]);
  });
  it("skips models with no version that fits", () => {
    const none = { ...model("x", 6, 90, 1e9), best: null } as unknown as ModelStatus;
    expect(runnable([none])).toEqual([]);
  });
});
