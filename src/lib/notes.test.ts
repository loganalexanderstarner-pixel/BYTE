import { describe, expect, it } from "vitest";

import { allTags, BOOKMARKLET, filterNotes, noteAge, noteFromAnswer, parseTags } from "./notes";
import type { Note } from "./types";

const note = (title: string, folder: string, tags: string[], body: string, updated: number): Note => ({ id: `${folder}/${title}.md`, title, folder, tags, created: updated, updated, source: "", chat: "", body, path: "" });
const notes = [note("Groceries", "Inbox", ["home"], "milk, eggs, coffee", 1), note("Coffee gear", "Home", ["home", "coffee"], "grinder", 2), note("Trip", "Travel", [], "pack the coffee", 3)];

describe("notes in the UI", () => {
  it("filters by words, folder and tag, title hits first", () => {
    expect(filterNotes(notes, { query: "coffee", folder: "", tag: "" }).map((n) => n.title)).toEqual(["Coffee gear", "Trip", "Groceries"]);
    expect(filterNotes(notes, { query: "", folder: "Home", tag: "" }).map((n) => n.title)).toEqual(["Coffee gear"]);
    expect(filterNotes(notes, { query: "milk eggs", folder: "", tag: "home" }).map((n) => n.title)).toEqual(["Groceries"]);
  });

  it("counts and parses tags", () => {
    expect(allTags(notes)).toEqual([["home", 2], ["coffee", 1]]);
    expect(parseTags("work, #Ideas,  travel, work")).toEqual(["work", "ideas", "travel"]);
  });

  it("makes notes from answers and labels ages", () => {
    const n = noteFromAnswer("## Plan\nGo early [1][2]. Bring water [3, 4].", "Hiking plan", ["outdoors"]);
    expect(n).toEqual({ title: "Hiking plan", folder: "Inbox", tags: ["outdoors"], body: "## Plan\nGo early. Bring water." });
    expect(noteFromAnswer("# Big title\ntext", "").title).toBe("Big title");
    expect(noteAge(1_000_000, 1_000_000 + 5 * 60_000)).toBe("5 min ago");
    expect(BOOKMARKLET.startsWith("javascript:location.href='byte://clip?url=")).toBe(true);
  });
});
