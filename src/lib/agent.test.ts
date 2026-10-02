import { describe, expect, it } from "vitest";

import { approveLabel, canOpen, endBrowsing, fileSize } from "./agent";
import type { ApprovalCard } from "./types";

const card = (id: string, status: ApprovalCard["status"]): ApprovalCard => ({
  id,
  status,
  action: "submit",
  title: "Submit the form on example.com?",
  site: "example.com",
  url: "https://example.com/",
  target: "Send",
  fields: [],
});

describe("web agent helpers", () => {
  it("expires cards nobody answered when the answer ends", () => {
    const m = { browsing: true, approvals: [card("a", "approved"), card("b", "waiting")] };
    const done = endBrowsing(m);
    expect(done.browsing).toBe(false);
    expect(done.approvals!.map((a) => a.status)).toEqual(["approved", "expired"]);
    const quiet = { browsing: false, approvals: [card("a", "declined")] };
    expect(endBrowsing(quiet)).toBe(quiet);
  });

  it("formats sizes and decides what opens directly", () => {
    expect(fileSize(2_500_000)).toBe("2.4 MB");
    expect(fileSize(10)).toBe("1 KB");
    const f = (name: string) => ({ path: `/x/${name}`, name, format: "download" as const, bytes: 1, url: "" });
    expect(canOpen(f("Guide.pdf"))).toBe(true);
    expect(canOpen(f("setup.dmg"))).toBe(false);
    expect(canOpen(f("run.sh"))).toBe(false);
    expect(approveLabel({ action: "download" })).toBe("Download");
    expect(approveLabel({ action: "submit" })).toBe("Submit");
    expect(approveLabel({ action: "mac" })).toBe("Do it");
  });
});
