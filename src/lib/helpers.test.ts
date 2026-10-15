import { describe, expect, it } from "vitest";

import { ASK_WHERE, defaultHelperKeys, helperKeys, helperRows } from "./helpers";

describe("helper settings rows", () => {
  it("has a row for each helper with its own switch, and Windows-only ones hide elsewhere", () => {
    expect(helperRows(true).map((r) => r.key)).toEqual(["helperRegion", "helperExplain", "helperDialogs", "helperSmartPaste", "helperDictate", "helperPage"]);
    expect(helperRows(false).map((r) => r.key)).toEqual(["helperExplain", "helperSmartPaste"]);
  });

  it("offers Local first and as the only default, then Auto and Cloud", () => {
    expect(ASK_WHERE.map((o) => o.id)).toEqual(["local", "auto", "cloud"]);
  });

  it("uses the saved keys, else the defaults for the system", () => {
    expect(helperKeys({}, true)).toEqual(defaultHelperKeys(true));
    expect(helperKeys({ helperRegionKeys: "Control+Shift+KeyY" }, true).helperRegionKeys).toBe("Control+Shift+KeyY");
    expect(defaultHelperKeys(false).helperRegionKeys).toBe("Alt+Super+KeyR");
    expect(defaultHelperKeys(true).helperRegionKeys).not.toBe(defaultHelperKeys(true).helperExplainKeys);
  });

  it("no row promises to send anything to the cloud on its own", () => {
    for (const r of helperRows(true)) expect(r.text.toLowerCase()).not.toContain("sends");
  });
});
