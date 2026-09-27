// Mirrors of the Rust types that cross the IPC boundary (see src-tauri/src).

export type Mode = "fast" | "auto" | "deep" | "extended";
export type ThinkingPref = "auto" | "on" | "off";

export interface Settings {
  onboardingComplete: boolean;
  activeModel: string | null;
  contextSize: number | null;
  defaultMode: Mode;
  thinking: ThinkingPref;
  theme: string;
  accent: string | null;
  fontScale: number;
  density: "comfortable" | "compact";
  showStats: boolean;
  webSearch: boolean;
  userName: string | null;
}

export interface SystemInfo {
  chip: string;
  totalRamBytes: number;
  gpuBudgetBytes: number;
  freeDiskBytes: number;
  osVersion: string;
  cpuCores: number;
  appleSilicon: boolean;
}

export type Fit = "great" | "tight" | "toobig";

export interface FitPlan {
  fit: Fit;
  context: number;
  neededBytes: number;
  gpuBudgetBytes: number;
  totalRamBytes: number;
  note: string;
}

export interface VariantStatus {
  /** "modelId:quant", e.g. "qwen3.8-27b:UD-IQ3_XXS". */
  key: string;
  quant: string;
  bits: number;
  sizeBytes: number;
  installed: boolean;
  partialBytes: number;
  downloading: boolean;
  /** Model quality adjusted for this quantization (0–100). */
  quality: number;
  fit: FitPlan;
  /** Smallest standard Mac memory size this version needs. */
  minRamGb: number;
}

export interface ModelStatus {
  id: string;
  name: string;
  family: string | null;
  released: string | null;
  tagline: string;
  tags: string[];
  thinking: boolean;
  tools: boolean;
  license: string | null;
  quality: number;
  repo: string;
  role: "chat" | "draft" | "embed";
  sizeLabel: string | null;
  maxContext: number;
  variants: VariantStatus[];
  /** Key of the best version for this Mac, or null if none fits. */
  best: string | null;
  minRamGb: number;
}

export type EngineStatus =
  | { state: "noModel" }
  | { state: "stopped" }
  | { state: "starting"; model: string }
  | { state: "ready"; model: string; context: number }
  | { state: "error"; message: string };

export type DownloadEvent =
  | { kind: "resuming"; id: string; bytes: number }
  | { kind: "progress"; id: string; bytes: number; total: number; bytesPerSec: number }
  | { kind: "verifying"; id: string }
  | { kind: "finished"; id: string }
  | { kind: "paused"; id: string; bytes: number }
  | { kind: "failed"; id: string; message: string };

export interface Stats {
  promptTokens: number;
  completionTokens: number;
  tokensPerSecond: number;
  promptMs: number;
  totalMs: number;
  thinkingMs: number;
}

export interface Source {
  n: number;
  title: string;
  url: string;
  snippet: string;
  /** True when BYTE actually read the page, not just saw it in results. */
  read: boolean;
}

export type ChatEvent =
  | { kind: "started"; thinking: boolean; model: string }
  | { kind: "reasoning"; delta: string }
  | { kind: "content"; delta: string }
  | { kind: "toolCall"; id: string; name: string; args: Record<string, unknown> }
  | { kind: "toolResult"; id: string; ok: boolean; summary: string }
  | { kind: "sources"; sources: Source[] }
  | ({ kind: "stats" } & Stats)
  | { kind: "done"; finishReason: string };

export interface WireMessage {
  role: "user" | "assistant";
  content: string;
}
