// The brainstorm board's state (pure; Board.tsx draws it, board.rs saves it and asks the model).
export const COLORS = ["yellow", "pink", "blue", "green", "purple"] as const;
export type StickyColor = (typeof COLORS)[number];

export interface Sticky {
  id: string;
  text: string;
  x: number;
  y: number;
  color: StickyColor;
}

export interface Group {
  id: string;
  label: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface BoardData {
  stickies: Sticky[];
  groups: Group[];
}

export const STICKY_W = 170;
export const STICKY_H = 110;
const GAP = 18;

let seq = 0;
export const newId = (p: string) => `${p}${Date.now().toString(36)}${(seq++).toString(36)}`;

export type BoardAction =
  | { type: "add"; text: string; x?: number; y?: number; color?: StickyColor }
  | { type: "addMany"; texts: string[]; near?: string }
  | { type: "move"; id: string; x: number; y: number }
  | { type: "edit"; id: string; text: string }
  | { type: "color"; id: string; color: StickyColor }
  | { type: "delete"; id: string }
  | { type: "group"; groups: [string, number[]][] }
  | { type: "ungroup" }
  | { type: "load"; data: BoardData };

/** A free spot: the first grid cell (left to right, top to bottom) no sticky overlaps. */
export function freeSpot(stickies: Sticky[], startX = 40, startY = 40, cols = 5): { x: number; y: number } {
  for (let i = 0; i < 500; i++) {
    const x = startX + (i % cols) * (STICKY_W + GAP);
    const y = startY + Math.floor(i / cols) * (STICKY_H + GAP);
    if (!stickies.some((s) => Math.abs(s.x - x) < STICKY_W && Math.abs(s.y - y) < STICKY_H)) return { x, y };
  }
  return { x: startX, y: startY };
}

export function boardReducer(b: BoardData, a: BoardAction): BoardData {
  switch (a.type) {
    case "load":
      return { stickies: a.data.stickies ?? [], groups: a.data.groups ?? [] };
    case "add": {
      const spot = a.x !== undefined && a.y !== undefined ? { x: a.x, y: a.y } : freeSpot(b.stickies);
      return { ...b, stickies: [...b.stickies, { id: newId("s"), text: a.text, ...spot, color: a.color ?? "yellow" }] };
    }
    case "addMany": {
      const near = b.stickies.find((s) => s.id === a.near);
      let stickies = b.stickies;
      for (const text of a.texts) {
        const spot = near ? freeSpot(stickies, near.x + STICKY_W + GAP, near.y, 2) : freeSpot(stickies);
        stickies = [...stickies, { id: newId("s"), text, ...spot, color: near?.color ?? "blue" }];
      }
      return { ...b, stickies };
    }
    case "move":
      return { ...b, stickies: b.stickies.map((s) => (s.id === a.id ? { ...s, x: Math.round(a.x), y: Math.round(a.y) } : s)) };
    case "edit":
      return { ...b, stickies: b.stickies.map((s) => (s.id === a.id ? { ...s, text: a.text.slice(0, 300) } : s)) };
    case "color":
      return { ...b, stickies: b.stickies.map((s) => (s.id === a.id ? { ...s, color: a.color } : s)) };
    case "delete":
      return { ...b, stickies: b.stickies.filter((s) => s.id !== a.id) };
    case "ungroup":
      return { ...b, groups: [] };
    case "group": {
      // Lay each theme out as a column of its stickies inside a labelled frame; ungrouped ones go last.
      const order = b.stickies;
      const placed = new Set<number>();
      const stickies = [...order];
      const groups: Group[] = [];
      let x = 40;
      const column = (label: string, idx: number[]) => {
        const y0 = 80;
        idx.forEach((i, k) => {
          stickies[i] = { ...stickies[i], x: x + 16, y: y0 + k * (STICKY_H + 12) };
          placed.add(i);
        });
        groups.push({ id: newId("g"), label, x, y: 40, w: STICKY_W + 32, h: 56 + idx.length * (STICKY_H + 12) });
        x += STICKY_W + 32 + GAP * 2;
      };
      for (const [label, idx] of a.groups) {
        const valid = idx.filter((i) => i >= 0 && i < order.length && !placed.has(i));
        if (valid.length) column(label, valid);
      }
      const rest = order.map((_, i) => i).filter((i) => !placed.has(i));
      if (rest.length) column("Other", rest);
      return { stickies, groups };
    }
  }
}

/** The board as a Markdown outline: one heading per group, its stickies as bullets. */
export function boardOutline(title: string, b: BoardData): string {
  const inside = (g: Group, s: Sticky) => s.x >= g.x && s.x <= g.x + g.w && s.y >= g.y && s.y <= g.y + g.h;
  const lines = [`# ${title}`, ""];
  const used = new Set<string>();
  for (const g of b.groups) {
    const items = b.stickies.filter((s) => inside(g, s)).sort((p, q) => p.y - q.y);
    if (!items.length) continue;
    lines.push(`## ${g.label}`, ...items.map((s) => `- ${s.text}`), "");
    items.forEach((s) => used.add(s.id));
  }
  const rest = b.stickies.filter((s) => !used.has(s.id)).sort((p, q) => p.y - q.y || p.x - q.x);
  if (rest.length) {
    if (b.groups.length) lines.push("## Other");
    lines.push(...rest.map((s) => `- ${s.text}`), "");
  }
  return lines.join("\n").trim() + "\n";
}
