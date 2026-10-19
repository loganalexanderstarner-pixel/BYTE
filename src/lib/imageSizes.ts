/** Picture shapes the Generate form offers. Sizes are multiples of 16, which every model accepts. */
export const SIZES: { id: string; label: string; w: number; h: number }[] = [
  { id: "square", label: "Square (1024 × 1024)", w: 1024, h: 1024 },
  { id: "portrait", label: "Portrait (896 × 1152)", w: 896, h: 1152 },
  { id: "tall", label: "Phone wallpaper (768 × 1344)", w: 768, h: 1344 },
  { id: "landscape", label: "Landscape (1216 × 832)", w: 1216, h: 832 },
  { id: "wide", label: "Wide (1344 × 768)", w: 1344, h: 768 },
  { id: "small", label: "Quick draft (512 × 512)", w: 512, h: 512 },
];

export function sizeOf(id: string): { w: number; h: number } {
  const s = SIZES.find((x) => x.id === id) ?? SIZES[1];
  return { w: s.w, h: s.h };
}

/** "3 min 20 s" style wait text from seconds. */
export function took(seconds: number): string {
  if (seconds < 90) return `${Math.round(seconds)} s`;
  return `${Math.floor(seconds / 60)} min ${Math.round(seconds % 60)} s`;
}
