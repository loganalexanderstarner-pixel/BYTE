import { describe, expect, it } from "vitest";

import { TABS } from "../components/settings/tabs";
import { ARTICLES } from "./help";
import {
  KEY,
  TUTORIALS,
  doAllQueue,
  linksOf,
  loadProgress,
  nextInRun,
  recommended,
  relatedNext,
  resumePoint,
  runPosition,
  saveProgress,
  searchTutorials,
  statusOf,
  stepsFor,
  tutorialsFor,
  withDone,
  withStep,
  type Store,
} from "./tutorial";

const groups = TABS.map((t) => t.id as string);
const articles = ARTICLES.map((a) => a.id);
const textOf = (os: "mac" | "windows") =>
  tutorialsFor(os)
    .flatMap((t) => [t.title, t.promise, ...stepsFor(t, os).flatMap((s) => [s.title, ...s.body, s.example ?? "", s.ifStuck ?? "", ...(s.links ?? []).map((l) => l.label), s.check?.label ?? ""])])
    .join("\n");

const memory = (): Store & { data: Record<string, string> } => {
  const data: Record<string, string> = {};
  return { data, getItem: (k) => data[k] ?? null, setItem: (k, v) => void (data[k] = v) };
};

describe("tutorial data", () => {
  it("has unique ids and 3 to 7 non-empty steps in every tutorial, on both systems", () => {
    expect(new Set(TUTORIALS.map((t) => t.id)).size).toBe(TUTORIALS.length);
    for (const os of ["mac", "windows"] as const)
      for (const t of tutorialsFor(os)) {
        const steps = stepsFor(t, os);
        expect(steps.length, `${t.id} on ${os}`).toBeGreaterThanOrEqual(3);
        expect(steps.length, `${t.id} on ${os}`).toBeLessThanOrEqual(7);
        expect(new Set(t.steps.map((s) => s.id)).size, t.id).toBe(t.steps.length);
        for (const s of steps) {
          expect(s.title.trim(), `${t.id}/${s.id}`).not.toBe("");
          expect(s.body.length, `${t.id}/${s.id}`).toBeGreaterThan(0);
          expect(s.body.every((b) => b.trim().length > 0)).toBe(true);
        }
        expect(t.promise.length).toBeGreaterThan(10);
        expect(t.minutes).toBeGreaterThan(0);
      }
  });

  it("every deep link points at a real settings group or help article", () => {
    for (const t of TUTORIALS)
      for (const l of linksOf(t)) {
        if (l.kind === "setting") expect(groups, `${t.id}: ${l.tab}`).toContain(l.tab);
        if (l.kind === "help") expect(articles, `${t.id}: ${l.id}`).toContain(l.id);
        expect(l.label.length).toBeGreaterThan(3);
      }
  });

  it("features that listen, look or control say how to turn them on", () => {
    for (const id of ["hey-byte", "pc-control", "screen", "find-files", "barge-in", "guided"]) {
      const t = TUTORIALS.find((x) => x.id === id)!;
      expect(t.offByDefault, id).toBe(true);
      expect(t.steps.map((s) => s.body.join(" ")).join(" ").toLowerCase(), id).toMatch(/turn(ed)? on|off until/);
    }
  });
});

describe("systems", () => {
  it("the Mac sees no Windows-only tutorial and no Windows-only words", () => {
    const ids = tutorialsFor("mac").map((t) => t.id);
    for (const w of ["pc-control", "screen", "find-files", "guided", "barge-in", "gaming"]) expect(ids).not.toContain(w);
    expect(textOf("mac")).not.toMatch(/Windows|\bPC\b|File Explorer|Ctrl\+|Credential Manager|Edge or Chrome|SmartScreen/);
  });

  it("Windows sees no Mac-only words and gets every tutorial", () => {
    expect(tutorialsFor("windows").length).toBe(TUTORIALS.length);
    expect(textOf("windows")).not.toMatch(/\bMac\b|macOS|Finder|Keychain|Option\+|Command\+|⌘|⌥/);
  });

  it("a step marked for one system is dropped on the other", () => {
    const quick = TUTORIALS.find((t) => t.id === "quick-ask")!;
    expect(stepsFor(quick, "windows").some((s) => s.example === "Ctrl+Alt+Space")).toBe(true);
    expect(stepsFor(quick, "mac").some((s) => s.example === "Ctrl+Alt+Space")).toBe(false);
  });

  it("the order is stable", () => {
    expect(TUTORIALS.map((t) => t.id)).toEqual([
      "getting-started", "chatting", "files", "web", "voice", "hey-byte", "barge-in", "quick-ask", "helpers",
      "pc-control", "find-files", "screen", "guided", "gaming", "backup", "privacy", "troubleshooting", "autonomous-mode",
    ]);
  });
});

describe("progress", () => {
  it("round-trips and survives broken storage", () => {
    const s = memory();
    saveProgress(s, withStep(withDone({ done: [], step: {}, all: { active: true, skipped: ["web"] } }, "files"), "voice", 2));
    const p = loadProgress(s);
    expect(p.done).toEqual(["files"]);
    expect(p.step).toEqual({ voice: 2 });
    expect(p.all).toEqual({ active: true, skipped: ["web"] });
    s.data[KEY] = "{not json";
    expect(loadProgress(s).done).toEqual([]);
    s.data[KEY] = JSON.stringify({ done: [1, "a"], step: { x: -3, y: "z", ok: 1.7 }, all: 5 });
    expect(loadProgress(s)).toEqual({ done: ["a"], step: { ok: 1 }, all: { active: false, skipped: [] } });
    const throwing: Store = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
    expect(loadProgress(throwing).done).toEqual([]);
    expect(() => saveProgress(throwing, p)).not.toThrow();
    expect(loadProgress(null).done).toEqual([]);
  });

  it("reports done, in progress and not started", () => {
    let p = loadProgress(null);
    expect(statusOf(p, "web")).toBe("new");
    p = withStep(p, "web", 2);
    expect(statusOf(p, "web")).toBe("progress");
    p = withDone(p, "web");
    expect(statusOf(p, "web")).toBe("done");
    expect(p.step.web).toBeUndefined();
  });
});

describe("recommended and do all", () => {
  const list = tutorialsFor("windows");
  it("recommends one in progress, else the first not done", () => {
    let p = loadProgress(null);
    expect(recommended(list, p)?.id).toBe("getting-started");
    p = withDone(p, "getting-started");
    expect(recommended(list, p)?.id).toBe("chatting");
    p = withStep(p, "voice", 1);
    expect(recommended(list, p)?.id).toBe("voice");
    for (const t of list) p = withDone(p, t.id);
    expect(recommended(list, p)).toBeNull();
  });

  it("do all visits each tutorial once, in order, without the skipped ones", () => {
    const q = doAllQueue(list, ["screen", "web"]);
    expect(q).toEqual(list.map((t) => t.id).filter((id) => id !== "screen" && id !== "web"));
    expect(new Set(q).size).toBe(q.length);
    expect(nextInRun(list, ["screen"], "find-files")).toBe("guided");
    expect(nextInRun(list, [], list[list.length - 1].id)).toBeNull();
    expect(runPosition(list, ["web"], "voice")).toEqual({ n: 4, of: list.length - 1 });
  });

  it("resumes where the run stopped and offers a related next tutorial", () => {
    let p = loadProgress(null);
    p = withDone(withDone(p, "getting-started"), "chatting");
    expect(resumePoint(list, p)).toBe("files");
    expect(resumePoint(list, { ...p, all: { active: true, skipped: ["files"] } })).toBe("web");
    expect(relatedNext(list, p, "files")?.id).toBe("web");
    expect(relatedNext(list, withDone(p, "web"), "files")?.id).toBe("voice");
  });

  it("search finds cards by words in the title, promise and steps", () => {
    expect(searchTutorials(list, "hands-free").map((t) => t.id)).toContain("hey-byte");
    expect(searchTutorials(list, "notepad").map((t) => t.id)).toEqual(["pc-control"]);
    expect(searchTutorials(list, "zzqx")).toEqual([]);
    expect(searchTutorials(list, "").length).toBe(list.length);
  });
});
