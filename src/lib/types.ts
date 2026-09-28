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
  /** A BYTE cloud key is saved in the Keychain (the key itself never reaches the UI). */
  cloudConnected: boolean;
  cloudBaseUrl: string | null;
  /** Raw `GET /api/auth/me`, cached. Use `cloudStatus()` for the parsed form. */
  cloudAccount: unknown;
  /** Answer with the BYTE cloud instead of this Mac. */
  useCloud: boolean;
  cloudMode: string | null;
  /** Sidebar workspace: this Mac, the BYTE cloud, or both answering together. */
  workspace: Workspace;
}

export type Workspace = "local" | "cloud" | "both";

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

/** How much memory macOS lets the GPU use (raising it needs the admin password; lasts until restart). */
export interface GpuShare {
  supported: boolean;
  currentBytes: number;
  defaultBytes: number;
  raisedBytes: number;
  raised: boolean;
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
  /** Conversation id on the BYTE cloud. */
  cloudId?: string | null;
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

/** An app and the memory it's using (Rust `memory::AppMemory`). */
export interface AppMemory {
  name: string;
  bytes: number;
  processes: number;
}

export interface MemoryReport {
  totalBytes: number;
  availableBytes: number;
  apps: AppMemory[];
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

/** What a model shows when opened (Rust `models::ModelDetails`). */
export interface ModelDetails {
  about: string;
  author: string | null;
  sourceUrl: string | null;
  /** chat, writing, coding, reasoning, math, languages, speed: 1–5 each (BYTE's estimate). */
  strengths: Record<string, number>;
  ideas: string[];
  community: boolean;
  caution: string | null;
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
  details?: ModelDetails | null;
  /** Can see photos once its image adapter (key "<id>:vision") is downloaded. */
  vision?: { key: string; sizeBytes: number; installed: boolean; downloading: boolean } | null;
}

export type EngineStatus =
  | { state: "noModel" }
  | { state: "stopped" }
  | { state: "starting"; model: string }
  | { state: "ready"; model: string; context: number; boosted: boolean; vision?: boolean }
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
  | { kind: "done"; finishReason: string }
  | { kind: "phase"; text: string }
  | { kind: "remote"; conversationId: string; messageId: string | null; userMessageId: string | null }
  | { kind: "notice"; text: string };

/** One mode the BYTE cloud offers this account (never a fixed list). */
export interface CloudMode {
  id: string;
  label: string;
}

export interface CloudMe {
  name: string | null;
  email: string | null;
  tier: string | null;
  modes: CloudMode[];
  budgets: unknown;
}

export interface CloudStatus {
  connected: boolean;
  baseUrl: string;
  account: CloudMe | null;
}

export interface WireMessage {
  role: "user" | "assistant";
  content: string;
  /** Files attached to a user message (read on this Mac). */
  files?: LocalFile[];
}

/** Rust `files::FileKind`. */
export type FileKind = "pdf" | "word" | "slides" | "sheet" | "text" | "web" | "image";

/** A file read on this Mac for a local chat (Rust `files::Ingested`). */
export interface LocalFile {
  name: string;
  kind: FileKind;
  /** Pages, slides or sheets. */
  pages?: number | null;
  text: string;
  truncated: boolean;
  /** Photos: a data URL (sent only to models that can see). */
  image?: string;
  /** The text was read from a scan or photo (text recognition). */
  ocr?: boolean;
}
