import { describe, expect, it } from "vitest";

import { boardOutline, boardReducer, freeSpot, STICKY_W, type BoardData } from "./board";

const empty: BoardData = { stickies: [], groups: [] };
const withThree = ["Tacos", "Iced coffee", "Burritos"].reduce((b, text) => boardReducer(b, { type: "add", text }), empty);

describe("brainstorm board", () => {
  it("adds stickies in free spots without overlap", () => {
    expect(withThree.stickies.length).toBe(3);
    const [a, b] = withThree.stickies;
    expect(Math.abs(a.x - b.x) >= STICKY_W || Math.abs(a.y - b.y) >= 100).toBe(true);
    expect(freeSpot([])).toEqual({ x: 40, y: 40 });
  });

  it("moves, edits, recolors and deletes", () => {
    const id = withThree.stickies[0].id;
    let b = boardReducer(withThree, { type: "move", id, x: 500.4, y: 300.6 });
    expect(b.stickies[0]).toMatchObject({ x: 500, y: 301 });
    b = boardReducer(b, { type: "edit", id, text: "Fish tacos" });
    b = boardReducer(b, { type: "color", id, color: "pink" });
    expect(b.stickies[0]).toMatchObject({ text: "Fish tacos", color: "pink" });
    expect(boardReducer(b, { type: "delete", id }).stickies.length).toBe(2);
  });

  it("adds ideas next to a sticky", () => {
    const near = withThree.stickies[2];
    const b = boardReducer(withThree, { type: "addMany", texts: ["Salsa bar", "Churros"], near: near.id });
    expect(b.stickies.length).toBe(5);
    expect(b.stickies[3].x).toBeGreaterThan(near.x);
  });

  it("groups into themes and outlines the board", () => {
    const g = boardReducer(withThree, { type: "group", groups: [["Food", [0, 2]], ["Drinks", [1, 0]]] });
    expect(g.groups.map((x) => x.label)).toEqual(["Food", "Drinks"]);
    expect(boardOutline("Food truck", g)).toBe("# Food truck\n\n## Food\n- Tacos\n- Burritos\n\n## Drinks\n- Iced coffee\n");
    expect(boardOutline("Plain", withThree)).toContain("- Tacos");
  });
});
