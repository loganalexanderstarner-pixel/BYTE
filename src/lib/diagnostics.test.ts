import { describe, expect, it, vi } from "vitest";
import { copyDiagnostics } from "./diagnostics";

describe("copyDiagnostics", () => {
  it("copies the report", async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    const r = await copyDiagnostics(async () => "BYTE diagnostics", write);
    expect(r).toEqual({ ok: true, text: "BYTE diagnostics" });
    expect(write).toHaveBeenCalledWith("BYTE diagnostics");
  });

  it("still returns the text when the clipboard refuses", async () => {
    const r = await copyDiagnostics(async () => "report", async () => Promise.reject(new Error("denied")));
    expect(r.ok).toBe(false);
    expect(r.text).toBe("report");
  });

  it("reports a failure to build the report", async () => {
    const r = await copyDiagnostics(() => Promise.reject(new Error("locked")), async () => {});
    expect(r).toEqual({ ok: false, text: "", reason: "locked" });
  });
});
