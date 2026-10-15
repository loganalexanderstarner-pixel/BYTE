/** Helpers for the Mac upkeep cards (storage chart, sizes). */

/** Sizes like Finder shows them (decimal units). */
export function sizeText(bytes: number): string {
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`;
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${Math.round(bytes / 1e6)} MB`;
  if (bytes >= 1e3) return `${Math.round(bytes / 1e3)} KB`;
  return `${bytes} bytes`;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Squarified treemap (Bruls et al.): lays `values` out in a `w`×`h` box as
 * rectangles whose areas match the values, keeping them close to square.
 * Returns one rectangle per value, in the input order (zero values get an empty one).
 */
export function treemap(values: number[], w: number, h: number): Rect[] {
  const out: Rect[] = values.map(() => ({ x: 0, y: 0, w: 0, h: 0 }));
  const total = values.reduce((a, b) => a + Math.max(0, b), 0);
  if (total <= 0 || w <= 0 || h <= 0) return out;
  const scale = (w * h) / total;
  const items = values.map((v, i) => ({ i, a: Math.max(0, v) * scale })).filter((it) => it.a > 0).sort((a, b) => b.a - a.a);
  let box: Rect = { x: 0, y: 0, w, h };
  const worst = (row: number[], side: number) => {
    const s = row.reduce((a, b) => a + b, 0);
    const mx = Math.max(...row);
    const mn = Math.min(...row);
    return Math.max((side * side * mx) / (s * s), (s * s) / (side * side * mn));
  };
  let row: typeof items = [];
  const place = (r: typeof items) => {
    const s = r.reduce((a, b) => a + b.a, 0);
    if (box.w >= box.h) {
      const cw = s / box.h;
      let y = box.y;
      for (const it of r) {
        const ch = it.a / cw;
        out[it.i] = { x: box.x, y, w: cw, h: ch };
        y += ch;
      }
      box = { x: box.x + cw, y: box.y, w: box.w - cw, h: box.h };
    } else {
      const ch = s / box.w;
      let x = box.x;
      for (const it of r) {
        const cw = it.a / ch;
        out[it.i] = { x, y: box.y, w: cw, h: ch };
        x += cw;
      }
      box = { x: box.x, y: box.y + ch, w: box.w, h: box.h - ch };
    }
  };
  for (const it of items) {
    const side = Math.min(box.w, box.h);
    const cur = row.map((r) => r.a);
    if (row.length === 0 || worst([...cur, it.a], side) <= worst(cur, side)) {
      row.push(it);
    } else {
      place(row);
      row = [it];
    }
  }
  if (row.length) place(row);
  return out;
}

/** How full the disk is, 0–100 (null when unknown). */
export function usedPercent(total: number, free: number): number | null {
  if (!total) return null;
  return Math.round(((total - free) / total) * 100);
}
