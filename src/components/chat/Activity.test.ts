import { describe, expect, it } from "vitest";

import type { Step } from "../../state/store";
import { activitySummary } from "./Activity";

const step = (name: string, status: Step["status"] = "ok"): Step => ({ id: Math.random().toString(36), name, args: {}, status });

describe("activity summary", () => {
  it("counts searches, pages actually read and the forecast", () => {
    expect(activitySummary([step("web_search"), step("read_page"), step("read_page", "error"), step("read_page")])).toBe("Searched the web · read 2 pages");
    expect(activitySummary([step("weather")])).toBe("Checked the forecast");
    expect(activitySummary([step("weather", "error"), step("web_search"), step("web_search")])).toBe("Searched the web 2 times");
  });
});
