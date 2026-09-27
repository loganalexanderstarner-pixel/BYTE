import type { ModelStatus, VariantStatus } from "./types";

/** Splits "qwen3.8-27b:UD-IQ3_XXS" into model id and quant. */
export function splitKey(key: string): { id: string; quant: string | null } {
  const i = key.indexOf(":");
  return i < 0 ? { id: key, quant: null } : { id: key.slice(0, i), quant: key.slice(i + 1) };
}

export function findVariant(models: ModelStatus[], key: string | null | undefined): { model: ModelStatus; variant: VariantStatus } | null {
  if (!key) return null;
  const { id, quant } = splitKey(key);
  const model = models.find((m) => m.id === id);
  if (!model) return null;
  const variant = model.variants.find((v) => v.quant === quant) ?? model.variants.find((v) => v.quant === "Q4_K_M") ?? model.variants[0];
  return variant ? { model, variant } : null;
}

/** "Qwen3.8 27B" or "Qwen3.8 27B · IQ3" for engine badges. */
export function displayName(models: ModelStatus[], key: string, withQuant = false): string {
  const hit = findVariant(models, key);
  if (!hit) return splitKey(key).id;
  return withQuant ? `${hit.model.name} · ${shortQuant(hit.variant.quant)}` : hit.model.name;
}

/** "UD-IQ3_XXS" → "IQ3_XXS". */
export function shortQuant(q: string): string {
  return q.replace(/^UD-/, "");
}

/** Plain-English quality label for a quantization. */
export function quantLabel(bits: number): string {
  if (bits >= 8) return "Full quality";
  if (bits >= 6) return "Near full quality";
  if (bits >= 4.5) return "High quality";
  if (bits >= 4) return "Good quality";
  if (bits >= 3) return "Reduced quality";
  return "Low quality, smallest";
}

export const RAM_TIERS = [8, 16, 24, 32, 48, 64, 96, 128] as const;

export const TAG_LABELS: Record<string, string> = {
  reasoning: "Reasoning",
  coding: "Coding",
  writing: "Writing",
  multilingual: "Languages",
  fast: "Fast",
  small: "Small",
  flagship: "Flagship",
};

/** Which fit bucket a model falls into on this Mac. */
export function fitGroup(m: ModelStatus): "great" | "tight" | "toobig" {
  const best = m.variants.find((v) => v.key === m.best);
  if (!best) return "toobig";
  return best.fit.fit === "great" ? "great" : "tight";
}
