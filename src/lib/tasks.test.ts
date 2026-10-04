import { describe, expect, it } from "vitest";

import { dueText, fromInput, sortTasks, toInput, topicList } from "./tasks";
import type { Task } from "./types";

const at = (y: number, m: number, d: number, h = 9, min = 0) => new Date(y, m - 1, d, h, min).getTime();
const task = (id: number, extra: Partial<Task> = {}): Task => ({ id, title: `t${id}`, notes: "", due: null, remindAt: null, repeat: "", doneAt: null, created: id, ...extra });

describe("tasks", () => {
  const now = at(2026, 9, 30, 10); // Wednesday
  it("says when things are due", () => {
    expect(dueText(null, now)).toBe("");
    expect(dueText(at(2026, 9, 30, 17), now)).toMatch(/^Today 5:00/);
    expect(dueText(at(2026, 10, 1, 9), now)).toMatch(/^Tomorrow 9:00/);
    expect(dueText(at(2026, 9, 30, 9), now)).toMatch(/^Overdue · Today/);
    expect(dueText(at(2026, 9, 28, 9), now)).toMatch(/^Overdue · /);
    expect(dueText(at(2026, 10, 20, 9), now)).not.toMatch(/Today|Tomorrow|Overdue/);
  });
  it("sorts overdue and soon first, undated next, done last", () => {
    const list = [task(1), task(2, { due: at(2026, 10, 3) }), task(3, { due: at(2026, 9, 1) }), task(4, { doneAt: now }), task(5, { doneAt: now + 1 }), task(6)];
    expect(sortTasks(list).map((t) => t.id)).toEqual([3, 2, 1, 6, 5, 4]);
  });
  it("round-trips the date input", () => {
    const t = at(2026, 10, 2, 17, 5);
    expect(toInput(t)).toBe("2026-10-02T17:05");
    expect(fromInput(toInput(t))).toBe(t);
    expect(fromInput("")).toBeNull();
    expect(toInput(null)).toBe("");
  });
  it("cleans the followed topics", () => {
    expect(topicList(" AI, Steelers,, ai , climate, space, F1, chess")).toEqual(["AI", "Steelers", "climate", "space", "F1"]);
  });
});
