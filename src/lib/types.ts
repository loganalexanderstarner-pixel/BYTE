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
  /** Free-form "About me", included in every conversation. */
  aboutMe: string | null;
  /** Use saved memories and let BYTE suggest new ones. */
  memoryEnabled: boolean;
  /** Models reloaded alongside the main one at launch. */
  loadedAlongside: string[];
  /** Speculative decoding with a small same-family helper model. */
  speedBoost: boolean;
  /** What BYTE favours when recommending a model version. */
  speedPref: "speed" | "balanced" | "quality";
  /** Measure and apply the fastest engine settings the first time a model loads. */
  autoTune: boolean;
  /** Measured best settings per model key. */
  tuning: Record<string, Tuning>;
}

/** Engine settings measured to be fastest for one model on this Mac. */
export interface Tuning {
  boost: boolean;
  kvF16: boolean;
  ubatch: number;
  tokensPerSec: number;
  promptPerSec: number;
  chip: string;
  testedAt: number;
  flashAttn: boolean;
  draftNMax: number;
  draftPMin: number;
  /** The thorough tune (more settings, ~5 minutes) was run. */
  thorough: boolean;
  /** Kind of helper the look-ahead was tuned for. */
  helperKind: HelperKind;
  /** Repeated-text guessing won. */
  ngram: boolean;
}

/** "draft": a separate small model; the others are the model's own speed-up head. */
export type HelperKind = "draft" | "mtp" | "eagle3" | "dspark";

export interface TuneProgress {
  model: string;
  step: number;
  total: number;
  label: string;
  done: boolean;
  /** When tuning several models: which one (1-based) of how many. */
  modelIndex: number;
  modelCount: number;
}

export interface BoostInfo {
  enabled: boolean;
  available: boolean;
  helperKey: string | null;
  helperName: string | null;
  helperBytes: number;
  installed: boolean;
  kind: HelperKind | null;
}



/** A saved chat in the sidebar (messages load when it's opened). */
export interface ConversationMeta {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
  folder: string | null;
  messageCount: number;
  /** One-line summary BYTE writes after the first answer. */
  summary: string | null;
  tags: string[];
  projectId: string | null;
}

/** Chats that share instructions. */
export interface Project {
  id: string;
  name: string;
  instructions: string;
  createdAt: number;
}

export interface Profile {
  id: string;
  name: string;
  createdAt: number;
}

export interface Profiles {
  active: string;
  profiles: Profile[];
}

export interface ChatSummary {
  title: string;
  summary: string;
  tags: string[];
}

export interface SearchHit {
  conversationId: string;
  messageId: string;
  title: string;
  /** Matching text with hits wrapped in «». */
  snippet: string;
  updatedAt: number;
}

export interface Memory {
  id: string;
  text: string;
  /** "user" (typed in Settings) or "chat" (suggested by BYTE, confirmed). */
  source: string;
  createdAt: number;
}

export interface SystemInfo {
  chip: string;
  totalRamBytes: number;
  gpuBudgetBytes: number;
  freeDiskBytes: number;
  osVersion: string;
  cpuCores: number;
  appleSilicon: boolean;
  chipInfo: ChipInfo;
}

export interface ChipInfo {
  name: string;
  /** 1 for M1 … 4 for M4 (0 if unknown). */
  generation: number;
  tier: "base" | "pro" | "max" | "ultra";
  gpuCores: number | null;
  bandwidthGbps: number;
  gpuTflops: number;
  neuralEngineTops: number;
  exact: boolean;
}

export interface SpeedEstimate {
  tokensPerSec: number;
  promptPerSec: number;
  /** Seconds for a typical answer. */
  replySecs: number;
  replyThinkingSecs: number;
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
  /** Expected speed on this Mac. */
  speed: SpeedEstimate;
  /** Fits in the memory left next to the models already running. */
  fitsAlongside: boolean;
  /** Writing speed measured on this Mac by tuning (tokens/sec). */
  measuredTps?: number | null;
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
  /** Total parameters, billions. */
  paramsB: number | null;
  /** Parameters used per token (mixture-of-experts), billions. */
  activeB: number | null;
  /** What it's good for, in plain words. */
  usedFor: string | null;
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
  | { state: "ready"; model: string; context: number; boosted: boolean }
  | { state: "error"; message: string };

/** A model running in memory (the main one, or one loaded alongside). */
export interface LoadedModel {
  key: string;
  primary: boolean;
  status: EngineStatus;
  context: number;
  neededBytes: number;
}

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
  /** Speed boost: tokens the helper guessed, and how many were kept. */
  draftTokens: number;
  draftAccepted: number;
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
