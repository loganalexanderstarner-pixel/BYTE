import { describe, expect, it } from "vitest";

import { bounds, layout, toMarkdown, wrap } from "./mindmap";
import type { MapNode } from "./types";

const n = (label: string, children: MapNode[] = []): MapNode => ({ label, children });
const big = n("Center", Array.from({ length: 6 }, (_, i) => n(`B${i}`, Array.from({ length: 6 }, (_, j) => n(`L${i}.${j}`)))));

describe("mind map layout", () => {
  it("places every node once, centre first", () => {
    const p = layout(big);
    expect(p.length).toBe(1 + 6 + 36);
    expect(p[0]).toMatchObject({ id: "0", x: 0, y: 0, branch: -1 });
    expect(new Set(p.map((x) => x.id)).size).toBe(p.length);
    // Children belong to their branch.
    expect(p.find((x) => x.id === "0.3.2")?.branch).toBe(3);
  });

  it("keeps leaves apart", () => {
    const leaves = layout(big).filter((x) => x.depth === 2);
    let closest = Infinity;
    for (let i = 0; i < leaves.length; i++)
      for (let j = i + 1; j < leaves.length; j++) closest = Math.min(closest, Math.hypot(leaves[i].x - leaves[j].x, leaves[i].y - leaves[j].y));
    expect(closest).toBeGreaterThan(50);
  });

  it("is deterministic and hides collapsed branches", () => {
    expect(layout(big)).toEqual(layout(big));
    const p = layout(big, new Set(["0.1"]));
    expect(p.some((x) => x.parent === "0.1")).toBe(false);
    expect(p.find((x) => x.id === "0.1")?.hasChildren).toBe(true);
  });

  it("exports Markdown and wraps labels", () => {
    expect(toMarkdown(n("Trip", [n("Pack", [n("Passport"), n("Charger", [n("USB-C")])]), n("Book")]))).toBe(
      "# Trip\n\n## Pack\n- Passport\n- Charger\n  - USB-C\n\n## Book\n",
    );
    expect(wrap("A fairly long label that needs wrapping", 12)).toEqual(["A fairly", "long label", "that needs"]);
    const b = bounds(layout(big));
    expect(b.w).toBeGreaterThan(600);
  });
});
