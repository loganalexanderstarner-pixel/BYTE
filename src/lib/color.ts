// Color math for themes: contrast like WCAG, and the same mixes tokens.css derives.
export type RGB = [number, number, number];

export const isHex = (h: string): boolean => /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(h.trim());

export function hex(h: string): RGB {
  const s = h.trim().replace("#", "");
  const full = s.length === 3 ? [...s].map((c) => c + c).join("") : s;
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16)) as RGB;
}

/** color-mix(in srgb, a p, b) */
export const mix = (a: RGB, b: RGB, p: number): RGB => a.map((v, i) => v * p + b[i] * (1 - p)) as RGB;

export function luminance([r, g, b]: RGB): number {
  const ch = (v: number) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b);
}

export function contrast(a: RGB, b: RGB): number {
  const [x, y] = [luminance(a), luminance(b)].sort((m, n) => n - m);
  return (x + 0.05) / (y + 0.05);
}

/** A light background (for picking the base: light or dark status colors and shadows). */
export const isLight = (bg: string): boolean => isHex(bg) && luminance(hex(bg)) > 0.4;
