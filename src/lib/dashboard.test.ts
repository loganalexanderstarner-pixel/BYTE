import { describe, expect, it } from "vitest";

import { bars, tiles } from "./dashboard";
import type { DashboardSummary } from "./types";

const empty: DashboardSummary = { todosDue: 0, todosOverdue: 0, todosOpen: 0, todos: [], coming: [], nextAutomation: null, changes: [], unread: 0, research: 0 };
const now = new Date(2026, 8, 30, 12, 0).getTime();

describe("dashboard tiles", () => {
  it("shows nothing for a new user", () => {
    expect(tiles(empty, [], now)).toEqual([]);
    expect(tiles(null, null, now)).toEqual([]);
  });

  it("shows what's there, warnings first-class", () => {
    const t = tiles(
      { ...empty, todosDue: 2, todosOverdue: 1, todos: ["Pay rent", "Call the bank"], coming: [{ name: "Netflix", when: "tomorrow", kind: "bill", overdue: false }], unread: 7, research: 3, nextAutomation: ["Morning AI news", new Date(2026, 9, 1, 8, 0).getTime()] },
      [["9:30 AM", "Dentist"]],
      now,
    );
    expect(t.map((x) => x.id)).toEqual(["today", "todos", "coming", "watch", "auto", "research"]);
    expect(t[1]).toMatchObject({ title: "To-dos · 1 overdue", tone: "warn", ask: "What's on my to-do list?" });
    expect(t[3].lines).toEqual(["7 new stories in your feeds"]);
    expect(t[4].lines[0]).toMatch(/^Morning AI news — tomorrow /);
    expect(t[5]).toMatchObject({ ask: null, lines: ["3 chats with sources"] });
  });

  it("scales bars", () => {
    expect(bars([0, 2, 4])).toEqual([0, 0.5, 1]);
    expect(bars([0, 0])).toEqual([0, 0]);
  });
});
