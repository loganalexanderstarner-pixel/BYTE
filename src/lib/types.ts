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
  /** With web on: "auto" searches when a question needs it, "always" searches for every real question. */
  webMode?: WebMode;
  userName: string | null;
  /** The user's town for "near me" questions, e.g. "Pittsburgh, PA". */
  homePlace?: string | null;
  /** Research depth: 0 Normal, 1 More, 2 Max. */
  researchDepth?: number;
  /** Free-form "About me", included in every conversation. */
  aboutMe: string | null;
  /** Use saved memories and let BYTE suggest new ones. */
  memoryEnabled: boolean;
  /** Knowledge base module ("My files"): index chosen folders and search them in answers. */
  kbEnabled: boolean;
  /** Kitchen module: recipes, meal plans, the recipe box. */
  kitchenEnabled?: boolean;
  /** Recipe measures: "us" (cups, spoons, °F; default) or "metric". */
  measureUnits?: "us" | "metric";
  /** Web agent module: BYTE may use a browser for the user (asks before submitting). */
  webAgentEnabled?: boolean;
  reviewsEnabled?: boolean;
  pricesEnabled?: boolean;
  gameHintsEnabled?: boolean;
  /** Check cited answers against their sources (Deep, Extended, fact-check). */
  selfCheck?: boolean;
  /** Three drafts and a majority vote for hard questions (Deep, Extended). */
  bestOfThree?: boolean;
  /** Flashcards, quizzes, tutor mode and the Study panel. */
  studyEnabled?: boolean;
  /** Photo helper: a small vision model describes photos for models that can't see them. */
  photoHelper?: boolean;
  /** The writing studio (✍️). */
  writingEnabled?: boolean;
  /** "Translate … into …" in chat, part by part. */
  translateEnabled?: boolean;
  /** Reuse the answer to a question asked (almost exactly) in the last week. */
  answerCache: boolean;
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
  /** Citation details for research papers (Rust `SourceMeta`). */
  meta?: SourceMeta;
}

export interface SourceMeta {
  authors: string[];
  year: number | null;
  /** Journal, conference, or "arXiv". */
  venue: string;
  doi: string | null;
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
  | { kind: "notice"; text: string }
  | ({ kind: "decision" } & Decision)
  | ({ kind: "places" } & PlacesFound)
  | ({ kind: "trip" } & TripPlan)
  | ({ kind: "recipe" } & Recipe)
  | ({ kind: "recipeIdeas" } & RecipeIdeas)
  | ({ kind: "mealPlan" } & MealPlan)
  | ({ kind: "video" } & VideoCard)
  | ({ kind: "approval" } & ApprovalAsk)
  | { kind: "approvalDone"; id: string; ok: boolean }
  | ({ kind: "saved" } & SavedFile)
  | { kind: "browsing"; active: boolean }
  | ({ kind: "reviews" } & Reviews)
  | ({ kind: "prices" } & Prices)
  | ({ kind: "hints" } & GameHints)
  | ({ kind: "selfCheck" } & SelfCheck)
  | ({ kind: "flashcards" } & Flashcards)
  | ({ kind: "quiz" } & Quiz);

/** Flashcards made in chat (Rust `study::Flashcards`). */
export interface Flashcards {
  title: string;
  cards: { front: string; back: string }[];
}

/** A multiple-choice quiz (Rust `study::Quiz`). */
export interface Quiz {
  title: string;
  questions: { question: string; choices: string[]; answer: number; explanation: string }[];
}

/** A saved deck (Rust `study::DeckSummary`). */
export interface DeckSummary {
  id: number;
  name: string;
  cards: number;
  due: number;
  new: number;
  created: number;
}

/** A card with its spaced-repetition schedule (Rust `study::StudyCard`). */
export interface StudyCard {
  id: number;
  deckId: number;
  front: string;
  back: string;
  ease: number;
  interval: number;
  reps: number;
  lapses: number;
  due: number;
}

/** What reviewers say (Rust `reviews::Reviews`). */
export interface Reviews {
  product: string;
  verdict: string;
  ratings: { site: string; value: number; best: number; count: number | null; n: number }[];
  pros: ReviewPoint[];
  cons: ReviewPoint[];
  bestFor: string[];
  skipIf: string[];
  read: number;
}
export interface ReviewPoint {
  text: string;
  sources: number[];
}

/** Prices read from store pages (Rust `prices::Prices`). */
export interface Prices {
  product: string;
  offers: Offer[];
  /** RFC 3339 time the pages were read. */
  checkedAt: string;
}
export interface Offer {
  store: string;
  title: string;
  price: number;
  currency: string;
  inStock: boolean | null;
  condition: string;
  url: string;
  n: number;
}

/** Spoiler-free game hints (Rust `games::Hints`). */
export interface GameHints {
  game: string;
  spot: string;
  hints: string[];
  solution: string;
  sources: number[];
}

/** Claims the answer's sources don't clearly back (Rust `selfcheck::SelfCheck`). */
export interface SelfCheck {
  checked: number;
  issues: { claim: string; sources: number[]; verdict: "partly" | "no"; note: string }[];
}

/** The web agent asks before submitting, committing or downloading (Rust `web_agent::ApprovalAsk`). */
export interface ApprovalAsk {
  id: string;
  action: "submit" | "download" | "click";
  title: string;
  site: string;
  url: string;
  /** The button's or link's label. */
  target: string;
  fields: { label: string; value: string }[];
}

/** An approval card on a message, with what the user decided. */
export interface ApprovalCard extends ApprovalAsk {
  status: "waiting" | "approved" | "declined" | "expired";
}

/** A file the web agent saved in Downloads/BYTE (Rust `web_agent::SavedFile`). */
export interface SavedFile {
  path: string;
  name: string;
  format: "download" | "pdf" | "image" | "archive" | "text";
  bytes: number;
  url: string;
}

/** A YouTube video's summary (Rust `youtube::VideoCard`). */
export interface VideoCard {
  id: string;
  title: string;
  channel: string;
  seconds: number;
  thumbnail: string;
  language: string;
  autoCaptions: boolean;
  tldr: string;
  keyPoints: { start: number; text: string }[];
  chapters: { start: number; title: string; summary: string }[];
}

/** A recipe card (Rust `kitchen::Recipe`). */
export interface Recipe {
  title: string;
  description: string;
  category: string;
  cuisine: string;
  servings: number;
  prepMin: number | null;
  cookMin: number | null;
  difficulty: string;
  equipment: string[];
  ingredients: Ingredient[];
  steps: RecipeStep[];
  tips: string[];
  substitutions: string[];
  storage: string;
  image: string;
  sourceUrl: string;
  sourceName: string;
  emoji: string;
}

export interface Ingredient {
  qty: number | null;
  unit: string;
  item: string;
  note: string;
  have: boolean;
}

export interface RecipeStep {
  text: string;
  minutes: number | null;
  cue: string;
}

export interface RecipeIdeas {
  have: string[];
  ideas: { title: string; description: string; minutes: number | null; missing: string[]; emoji: string }[];
}

export interface MealPlan {
  days: { day: string; meals: { meal: string; title: string; description: string; minutes: number | null; emoji: string }[] }[];
  grocery: { aisle: string; items: string[] }[];
  have: string[];
}

/** A recipe in the recipe box (Rust `kitchen::SavedRecipe`). */
export interface SavedRecipe {
  id: number;
  title: string;
  category: string;
  image: string;
  savedAt: number;
  recipe: Recipe;
}

/** A place nearby from OpenStreetMap (Rust `tools::places::Spot`). */
export interface Spot {
  name: string;
  kind: string;
  address: string;
  lat: number;
  lon: number;
  distanceM: number;
  hours: string;
  openNow: boolean | null;
  website: string;
  phone: string;
  cuisine: string;
  osmUrl: string;
}

/** Places found near somewhere (Rust `tools::PlacesFound`). */
export interface PlacesFound {
  near: string;
  what: string;
  imperial: boolean;
  spots: Spot[];
}

/** A trip plan (Rust `trip::TripPlan`). */
export interface TripPlan {
  destination: string;
  currency: string;
  budget: number | null;
  travelers: number;
  month: string | null;
  days: TripDay[];
  costs: { category: string; amount: number }[];
  packing: string[];
  tips: string[];
  weather: string;
}

export interface TripDay {
  title: string;
  date: string | null;
  items: TripItem[];
}

export interface TripItem {
  time: string;
  title: string;
  place: string;
  note: string;
  cost: number | null;
  sources: number[];
}

/** A job asked for with a button (Rust `agent::Task`). */
export type ChatTask = "factCheck" | "browse" | "tutor";

/** Compare & decide score table (Rust `decide::Decision`): `scores[option][criterion]`. */
export interface Decision {
  options: string[];
  criteria: { name: string; weight: number }[];
  scores: (DecisionCell | null)[][];
}

export interface DecisionCell {
  score: number;
  reason: string;
  sources: number[];
}

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

/** A folder in the knowledge base (Rust `kb::Source`). */
export interface KbSource {
  id: number;
  path: string;
  addedAt: number;
  lastScan: number | null;
  error: string | null;
  files: number;
  chunks: number;
  /** Passages searchable by meaning (have an embedding). */
  embedded: number;
  bytes: number;
}

/** Rust `commands::LookerStatus`: the photo helper. */
export interface LookerStatus {
  /** The helper model in use, when one is downloaded. */
  model: string | null;
  /** What to download for the default helper (model, then its image adapter). */
  downloads: string[];
  downloadBytes: number;
}

/** Rust `commands::KbStatus`. */
export interface KbStatus {
  sources: KbSource[];
  embedKey: string | null;
  embedBytes: number;
  embedInstalled: boolean;
  embedRunning: boolean;
}

/** A passage found in the knowledge base (Rust `kb::Hit`). */
export interface KbHit {
  chunkId: number;
  path: string;
  name: string;
  page: number | null;
  text: string;
}

/** Indexing progress (`kb://progress`). */
export interface KbProgress {
  sourceId: number;
  phase: "reading" | "embedding" | "done";
  done: number;
  total: number;
  file: string;
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

/** Web modes shown on the Web pill: off, or on in one of two ways. */
export type WebMode = "auto" | "always";
