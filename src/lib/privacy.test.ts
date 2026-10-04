import { describe, expect, it } from "vitest";

import { argsLine, backupAge, byDay, formatBytes, kindLabel, lockAfterLabel, toolLabel } from "./privacy";
import type { Activity } from "./types";

const at = (ts: string, tool = "web_search"): Activity => ({ ts, tool, kind: "web", ok: true, summary: "", args: {} });

describe("activity log", () => {
  it("names tools plainly", () => {
    expect(toolLabel("web_search")).toBe("Searched the web");
    expect(toolLabel("mac_reminder_add")).toBe("Reminder add");
    expect(toolLabel("")).toBe("Something");
    expect(kindLabel("terminal")).toBe("Terminal");
  });

  it("groups by day, newest first", () => {
    const now = new Date(2026, 9, 2, 15, 0);
    const groups = byDay(
      [at(new Date(2026, 9, 2, 14).toISOString()), at(new Date(2026, 9, 2, 9).toISOString()), at(new Date(2026, 9, 1, 20).toISOString()), at(new Date(2026, 8, 28, 8).toISOString()), at("garbage")],
      now,
    );
    expect(groups.map((g) => [g.day, g.items.length])).toEqual([
      ["Today", 2],
      ["Yesterday", 1],
      [new Date(2026, 8, 28).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" }), 1],
      ["Earlier", 1],
    ]);
  });

  it("shows the meaningful argument", () => {
    expect(argsLine({ query: "best tents", max: 5 })).toBe("best tents");
    expect(argsLine({ command: "df -h /" })).toBe("df -h /");
    expect(argsLine(null)).toBe("");
    expect(argsLine({ n: 3 })).toBe("");
  });

  it("describes the lock timing", () => {
    expect(lockAfterLabel(0)).toBe("Only when BYTE opens");
    expect(lockAfterLabel(15)).toBe("After 15 minutes idle");
    expect(lockAfterLabel(60)).toBe("After an hour idle");
  });

  it("formats backup sizes and ages", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(48_000)).toBe("47 KB");
    expect(formatBytes(12_500_000)).toBe("11.9 MB");
    const now = Date.UTC(2026, 9, 2, 12);
    expect(backupAge(now - 3600_000, now)).toBe("today");
    expect(backupAge(now - 30 * 3600_000, now)).toBe("yesterday");
    expect(backupAge(now - 4 * 86_400_000, now)).toBe("4 days ago");
  });
});
