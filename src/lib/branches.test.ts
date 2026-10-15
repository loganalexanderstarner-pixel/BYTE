import { describe, expect, it } from "vitest";

import { branchAt, switchVersion, versionInfo, versionsAt, type Versioned } from "./branches";

type M = Versioned & { text: string };
const m = (id: string): M => ({ id, text: id });
const ids = (list: M[]) => list.map((x) => x.id);

describe("message versions", () => {
  const thread = [m("q1"), m("a1"), m("q2"), m("a2")];

  it("keeps the old thread when a message is edited", () => {
    const edited = branchAt(thread, 2, m("q2b"));
    expect(ids(edited)).toEqual(["q1", "a1", "q2b"]);
    expect(versionInfo(edited[2])).toEqual({ count: 2, index: 1 });
    expect(versionsAt(edited, 2).map(ids)).toEqual([["q2", "a2"], ["q2b"]]);
  });

  it("switches between versions and back without losing any", () => {
    const edited = [...branchAt(thread, 2, m("q2b")), m("a2b")];
    const back = switchVersion(edited, 2, 0);
    expect(ids(back)).toEqual(["q1", "a1", "q2", "a2"]);
    expect(versionInfo(back[2])).toEqual({ count: 2, index: 0 });
    const forward = switchVersion(back, 2, 1);
    expect(ids(forward)).toEqual(["q1", "a1", "q2b", "a2b"]);
    // A third version goes at the end.
    const third = branchAt(forward, 2, m("q2c"));
    expect(versionsAt(third, 2).map(ids)).toEqual([["q2", "a2"], ["q2b", "a2b"], ["q2c"]]);
    expect(versionInfo(third[2])).toEqual({ count: 3, index: 2 });
  });

  it("regenerating an answer keeps the earlier answers", () => {
    const again = branchAt(thread, 3, m("a2-new"));
    expect(ids(again)).toEqual(["q1", "a1", "q2", "a2-new"]);
    expect(ids(switchVersion(again, 3, 0))).toEqual(["q1", "a1", "q2", "a2"]);
  });

  it("appending past the end just adds the message", () => {
    expect(ids(branchAt(thread, 4, m("x")))).toEqual(["q1", "a1", "q2", "a2", "x"]);
  });
});
