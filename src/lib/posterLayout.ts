/** Poster layout for poems and quotes: where the words go on a picture and how they are set. Pure, so it is tested without a canvas. */

export type Side = "left" | "right" | "top" | "bottom";

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** The quietest part of the picture (dark and even) is where words read best. `grid` is brightness 0-255, row by row. */
export function quietestSide(grid: number[], cols: number, rows: number): Side {
  const at = (c: number, r: number) => grid[r * cols + c] ?? 128;
  const cut = (c0: number, c1: number, r0: number, r1: number) => {
    const v: number[] = [];
    for (let r = r0; r < r1; r++) for (let c = c0; c < c1; c++) v.push(at(c, r));
    const mean = v.reduce((a, b) => a + b, 0) / Math.max(1, v.length);
    const sd = Math.sqrt(v.reduce((a, b) => a + (b - mean) ** 2, 0) / Math.max(1, v.length));
    // Dark and even beats bright and busy.
    return mean + 0.8 * sd;
  };
  const wc = Math.max(1, Math.round(cols * 0.4));
  const hr = Math.max(1, Math.round(rows * 0.4));
  const scores: [Side, number][] = [
    ["left", cut(0, wc, 0, rows)],
    ["right", cut(cols - wc, cols, 0, rows)],
    ["top", cut(0, cols, 0, hr) + 6],
    ["bottom", cut(0, cols, rows - hr, rows) + 6],
  ];
  return scores.sort((a, b) => a[1] - b[1])[0][0];
}

/** Whether light text suits a part of the picture. */
export function wantsLightText(grid: number[], cols: number, rows: number, side: Side): boolean {
  let sum = 0;
  let n = 0;
  for (let r = 0; r < rows; r++)
    for (let c = 0; c < cols; c++) {
      const inside = side === "left" ? c < cols * 0.45 : side === "right" ? c >= cols * 0.55 : side === "top" ? r < rows * 0.45 : r >= rows * 0.55;
      if (inside) (sum += grid[r * cols + c] ?? 128), n++;
    }
  return sum / Math.max(1, n) < 140;
}

/** The area the words may use on a picture `w` × `h`. */
export function regionFor(side: Side, w: number, h: number): Box {
  const m = Math.round(Math.min(w, h) * 0.06);
  switch (side) {
    case "left":
      return { x: m, y: m, w: Math.round(w * 0.42) - m, h: h - 2 * m };
    case "right":
      return { x: Math.round(w * 0.58), y: m, w: Math.round(w * 0.42) - m, h: h - 2 * m };
    case "top":
      return { x: m, y: m, w: w - 2 * m, h: Math.round(h * 0.42) - m };
    default:
      return { x: m, y: Math.round(h * 0.58), w: w - 2 * m, h: Math.round(h * 0.42) - m };
  }
}

/** Breaks long lines to fit `maxW`, keeping the poem's own line breaks and blank lines between stanzas. */
export function wrapLines(text: string, measure: (s: string) => number, maxW: number): string[] {
  const out: string[] = [];
  for (const raw of text.replace(/\r/g, "").split("\n")) {
    const line = raw.trimEnd();
    if (!line.trim()) {
      out.push("");
      continue;
    }
    if (measure(line) <= maxW) {
      out.push(line);
      continue;
    }
    let cur = "";
    for (const word of line.split(/\s+/)) {
      const t = cur ? `${cur} ${word}` : word;
      if (cur && measure(t) > maxW) {
        out.push(cur);
        cur = "  " + word; // a wrapped line is indented so it reads as a continuation
      } else cur = t;
    }
    if (cur) out.push(cur);
  }
  while (out.length && out[0] === "") out.shift();
  while (out.length && out[out.length - 1] === "") out.pop();
  return out;
}

/** Splits lines into `n` columns, breaking at the blank line (stanza end) nearest each cut. */
export function splitColumns(lines: string[], n: number): string[][] {
  if (n <= 1) return [lines];
  const cols: string[][] = [];
  let start = 0;
  for (let i = 1; i <= n; i++) {
    if (i === n) {
      cols.push(lines.slice(start));
      break;
    }
    const ideal = Math.round((lines.length * i) / n);
    let best = ideal;
    for (let d = 0; d <= 3; d++) {
      if (lines[ideal + d] === "") best = ideal + d;
      else if (lines[ideal - d] === "") best = ideal - d;
      else continue;
      break;
    }
    cols.push(lines.slice(start, best));
    start = best;
  }
  return cols.map((c) => {
    const l = [...c];
    while (l[0] === "") l.shift();
    while (l[l.length - 1] === "") l.pop();
    return l;
  });
}

export interface Fitted {
  size: number;
  lineHeight: number;
  columns: string[][];
  colWidth: number;
  gap: number;
}

/**
 * The biggest type size at which the poem fits the region, using as few columns as it needs (up to 3).
 * `measure(text, size)` is the width of a line at a font size.
 */
export function fitPoem(text: string, region: Box, measure: (s: string, size: number) => number, maxSize = 64, minSize = 14, leading = 1.45): Fitted | null {
  const source = text.replace(/\r/g, "").split("\n");
  // First choice: the poet's own line breaks, untouched. Only when no size keeps them is a long line wrapped.
  for (const keepBreaks of [true, false]) {
    for (let size = maxSize; size >= minSize; size -= 2) {
      for (let n = 1; n <= 3; n++) {
        const gap = Math.round(size * 1.6);
        const colWidth = (region.w - gap * (n - 1)) / n;
        if (colWidth < size * 8) continue; // too narrow to read
        if (keepBreaks && source.some((l) => measure(l.trimEnd(), size) > colWidth)) continue;
        const lines = wrapLines(text, (s) => measure(s, size), colWidth);
        const columns = splitColumns(lines, n);
        const tallest = Math.max(...columns.map((c) => c.length));
        if (tallest * size * leading <= region.h) return { size, lineHeight: size * leading, columns, colWidth, gap };
      }
    }
  }
  return null;
}

export interface PosterStyle {
  id: string;
  label: string;
  hint: string;
}

export const POSTER_STYLES: PosterStyle[] = [
  { id: "typewriter", label: "Typewriter", hint: "Typed lines in columns, like a page from a machine" },
  { id: "elegant", label: "Elegant", hint: "Serif stanzas with a gentle indent" },
  { id: "neon", label: "Neon sign", hint: "Glowing red letters" },
  { id: "paint", label: "Dripping paint", hint: "Heavy painted letters with drips" },
  { id: "script", label: "Hand lettered", hint: "Flowing handwriting" },
];

/** The font stack for each style (Windows has all of these; the fallbacks cover other systems). */
export function fontFor(style: string, size: number): string {
  switch (style) {
    case "typewriter":
      return `${size}px "Courier New", "Courier Prime", monospace`;
    case "elegant":
      return `italic ${size}px Georgia, "Palatino Linotype", serif`;
    case "neon":
      return `bold ${size}px "Arial Black", "Segoe UI Black", sans-serif`;
    case "paint":
      return `${size}px Impact, "Arial Narrow Bold", sans-serif`;
    default:
      return `${size}px "Segoe Script", "Brush Script MT", cursive`;
  }
}
