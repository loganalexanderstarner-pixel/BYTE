import { describe, expect, it } from "vitest";

import { quantLabel, shortQuant, splitKey } from "./models";

describe("model helpers", () => {
  it("splits keys", () => {
    expect(splitKey("qwen3.8-27b:UD-IQ3_XXS")).toEqual({ id: "qwen3.8-27b", quant: "UD-IQ3_XXS" });
    expect(splitKey("qwen3-14b")).toEqual({ id: "qwen3-14b", quant: null });
  });
  it("labels quantization quality", () => {
    expect(quantLabel(8.5)).toBe("Full quality");
    expect(quantLabel(4.8)).toBe("High quality");
    expect(quantLabel(2.1)).toBe("Low quality, smallest");
    expect(shortQuant("UD-Q4_K_M")).toBe("Q4_K_M");
  });
});
