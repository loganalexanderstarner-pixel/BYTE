import { describe, expect, it } from "vitest";

import type { Conversation } from "../state/store";
import { inWorkspace, onlyOnCloud, sidebarSections } from "./Sidebar";

const conv = (id: string, extra: Partial<Conversation> = {}): Conversation => ({
  id,
  title: id,
  createdAt: 0,
  updatedAt: Date.now(),
  messages: [],
  messageCount: 2,
  ...extra,
});

describe("sidebar sections", () => {
  it("puts pinned first, then folders, then chats by date", () => {
    const s = sidebarSections([
      conv("a"),
      conv("b", { pinned: true }),
      conv("c", { folder: "Work" }),
      conv("d", { folder: "Work", updatedAt: Date.now() + 1 }),
      conv("e", { folder: "Home" }),
      conv("empty", { messageCount: 0 }),
      conv("p", { private: true, messages: [{ id: "m", role: "user", content: "hi", status: "done", createdAt: 0 }], messageCount: 0 }),
      conv("k", { projectId: "proj" }),
      conv("orphan", { projectId: "deleted-project" }),
    ], [{ id: "proj", name: "Kitchen", instructions: "", createdAt: 0 }]);
    expect(s.projects.map((p) => [p.project.name, p.items.map((c) => c.id)])).toEqual([["Kitchen", ["k"]]]);
    expect(s.pinned.map((c) => c.id)).toEqual(["b"]);
    expect(s.folders.map((f) => [f.name, f.items.map((c) => c.id)])).toEqual([
      ["Home", ["e"]],
      ["Work", ["d", "c"]],
    ]);
    expect(s.dated.map((g) => [g.label, g.items.map((c) => c.id)])).toEqual([
      ["Today", ["a", "orphan"]],
      ["Private (not saved)", ["p"]],
    ]);
  });
});

describe("workspace filtering", () => {
  it("shows each workspace its own chats; private chats stay on This Mac", () => {
    const all = [conv("a"), conv("cloud-1", { cloudId: "1" }), conv("both-x"), conv("p", { private: true })];
    expect(inWorkspace(all, "local").map((c) => c.id)).toEqual(["a", "p"]);
    expect(inWorkspace(all, "cloud").map((c) => c.id)).toEqual(["cloud-1"]);
    expect(inWorkspace(all, "both").map((c) => c.id)).toEqual(["both-x"]);
  });

  it("lists only cloud chats not already open on this Mac", () => {
    const cloud = [
      { id: "1", title: "one", updatedAt: 0 },
      { id: "2", title: "two", updatedAt: 0 },
    ];
    expect(onlyOnCloud(cloud, [conv("cloud-1", { cloudId: "1" })]).map((c) => c.id)).toEqual(["2"]);
    expect(onlyOnCloud(null, [])).toEqual([]);
  });
});
