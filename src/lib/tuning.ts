import type { LabModel, ModelOverride } from "./types";

/** Theme tone for a model-lab fit: great → ok, tight → warn, no → danger. */
export function fitTone(fit: LabModel["fit"]): "ok" | "warn" | "danger" {
  return fit === "great" ? "ok" : fit === "tight" ? "warn" : "danger";
}

/** Plain label for a fit. */
export function fitLabel(fit: LabModel["fit"]): string {
  return fit === "great" ? "Fits well" : fit === "tight" ? "Tight fit" : "Too big for this Mac";
}

/** 8.03 → "8.0B", 0.6 → "0.6B", 235 → "235B", null → "Unknown size". */
export function paramsLabel(paramsB: number | null | undefined): string {
  if (paramsB == null || !Number.isFinite(paramsB) || paramsB <= 0) return "Unknown size";
  return paramsB >= 100 ? `${Math.round(paramsB)}B` : `${paramsB.toFixed(1)}B`;
}

/** Thinking budget choices shown in the tuning panel (null = the model's recommended value). */
export const BUDGET_OPTIONS: (number | null)[] = [null, 256, 512, 1024, 2048, 4096, -1];

/** null → "Recommended", -1 → "No limit", 1024 → "1,024 tokens". */
export function budgetLabel(n: number | null | undefined): string {
  if (n == null) return "Recommended";
  if (n < 0) return "No limit";
  return `${n.toLocaleString("en-US")} tokens`;
}

/** used / total as a whole percentage, clamped to 0–100 (0 when total is unknown). */
export function meterPercent(used: number | null | undefined, total: number | null | undefined): number {
  if (!used || !total || !Number.isFinite(used) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((used / total) * 100)));
}

/** True when an override changes nothing (every field null/undefined or blank). */
export function isEmptyOverride(o: ModelOverride | null | undefined): boolean {
  if (!o) return true;
  return o.temperature == null && o.topP == null && o.thinkingBudget == null && !(o.systemExtra ?? "").trim();
}

/** Keeps only the fields that change something. */
function clean(o: ModelOverride): ModelOverride {
  const out: ModelOverride = {};
  if (o.temperature != null) out.temperature = o.temperature;
  if (o.topP != null) out.topP = o.topP;
  if (o.thinkingBudget != null) out.thinkingBudget = o.thinkingBudget;
  if ((o.systemExtra ?? "").trim()) out.systemExtra = o.systemExtra;
  return out;
}

/** A new overrides map with `patch` merged into `key`; the key is dropped when nothing is left to override. */
export function withOverride(
  overrides: Record<string, ModelOverride> | null | undefined,
  key: string,
  patch: Partial<ModelOverride>,
): Record<string, ModelOverride> {
  const merged = clean({ ...(overrides?.[key] ?? {}), ...patch });
  if (isEmptyOverride(merged)) return withoutOverride(overrides, key);
  return { ...(overrides ?? {}), [key]: merged };
}

/** A new overrides map without `key`. */
export function withoutOverride(overrides: Record<string, ModelOverride> | null | undefined, key: string): Record<string, ModelOverride> {
  const out = { ...(overrides ?? {}) };
  delete out[key];
  return out;
}
