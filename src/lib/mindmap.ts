// Mind map layout and exports (pure; MindMap.tsx draws it). A radial tree: main branches around the centre,
// each getting a share of the circle by how many leaves it has; children fan out further from the centre.
import type { MapNode } from "./types";

export interface Placed {
  id: string;
  label: string;
  depth: number;
  /** Which main branch it belongs to (for its colour); -1 for the centre. */
  branch: number;
  x: number;
  y: number;
  parent: string | null;
  hasChildren: boolean;
}

const RINGS = [0, 190, 330, 450];

const leaves = (n: MapNode, hidden: Set<string>, id: string): number =>
  hidden.has(id) || !n.children.length ? 1 : n.children.reduce((s, c, i) => s + leaves(c, hidden, `${id}.${i}`), 0);

/** Positions for every visible node (ids are index paths: "0", "0.2", "0.2.1"). Collapsed ids hide their children. */
export function layout(root: MapNode, hidden: Set<string> = new Set()): Placed[] {
  const out: Placed[] = [{ id: "0", label: root.label, depth: 0, branch: -1, x: 0, y: 0, parent: null, hasChildren: root.children.length > 0 }];
  const total = leaves(root, hidden, "0");
  const place = (n: MapNode, id: string, depth: number, from: number, to: number, branch: number) => {
    if (hidden.has(id)) return;
    const sum = leaves(n, hidden, id);
    let a = from;
    n.children.forEach((c, i) => {
      const cid = `${id}.${i}`;
      const span = ((to - from) * leaves(c, hidden, cid)) / sum;
      const mid = a + span / 2;
      const r = RINGS[Math.min(depth + 1, RINGS.length - 1)] + (depth + 1 >= RINGS.length ? 110 * (depth + 2 - RINGS.length) : 0);
      const b = depth === 0 ? i : branch;
      out.push({ id: cid, label: c.label, depth: depth + 1, branch: b, x: Math.cos(mid) * r, y: Math.sin(mid) * r, parent: id, hasChildren: c.children.length > 0 });
      place(c, cid, depth + 1, a, a + span, b);
      a += span;
    });
  };
  // Start at the top, going clockwise.
  if (total > 0) place(root, "0", 0, -Math.PI / 2, (3 * Math.PI) / 2, -1);
  return out;
}

/** The outline as Markdown (for copying and notes). */
export function toMarkdown(root: MapNode): string {
  const lines = [`# ${root.label}`, ""];
  const walk = (n: MapNode, depth: number) => {
    for (const c of n.children) {
      lines.push(depth === 0 ? `## ${c.label}` : `${"  ".repeat(depth - 1)}- ${c.label}`);
      walk(c, depth + 1);
      if (depth === 0) lines.push("");
    }
  };
  walk(root, 0);
  return lines.join("\n").trim() + "\n";
}

/** The smallest box around all nodes, with room for labels. */
export function bounds(nodes: Placed[], pad = 120): { x: number; y: number; w: number; h: number } {
  const xs = nodes.map((n) => n.x);
  const ys = nodes.map((n) => n.y);
  const x = Math.min(...xs) - pad;
  const y = Math.min(...ys) - pad / 2;
  return { x, y, w: Math.max(...xs) + pad - x, h: Math.max(...ys) + pad / 2 - y };
}

/** Labels wrapped to lines of about `width` characters (SVG has no wrapping). */
export function wrap(label: string, width = 16): string[] {
  const out: string[] = [];
  let cur = "";
  for (const w of label.split(/\s+/)) {
    if (cur && (cur + " " + w).length > width) {
      out.push(cur);
      cur = w;
    } else cur = cur ? `${cur} ${w}` : w;
  }
  if (cur) out.push(cur);
  return out.slice(0, 3);
}
