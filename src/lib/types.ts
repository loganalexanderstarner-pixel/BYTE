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
  /** The user's writing style for "Write like me". */
  writingStyle?: string;
  /** Advanced tuning per model key ("id:quant"): sampling, thinking budget, extra instructions. */
  modelOverrides?: Record<string, ModelOverride>;
  /** Battery saver: below 20% and unplugged, lighter modes and shorter thinking. */
  batterySaver?: boolean;
  /** "Translate … into …" in chat, part by part. */
  translateEnabled?: boolean;
  /** Mac control: notes, reminders, calendar, music, settings (macOS). */
  macControl?: boolean;
  macUpkeep?: boolean;
  tasksEnabled?: boolean;
  briefingTopics?: string[];
  watchEnabled?: boolean;
  /** Automations and multi-step runs (✅ panel). */
  automationsEnabled?: boolean;
  /** Packages, bills and subscriptions, birthdays, maintenance (✅ panel). */
  trackersEnabled?: boolean;
  /** Obsidian, Notion, calendar links (each off until set up). */
  connectorsEnabled?: boolean;
  obsidianVault?: string | null;
  notionParent?: string | null;
  /** Open BYTE in the background at login. */
  openAtLogin?: boolean;
  /** Closing the window keeps BYTE running (macOS); ⌘Q quits. */
  keepRunning?: boolean;
  /** ⌥⌘B opens selected text from any app in the writing studio. */
  selectionHotkey?: boolean;
  /** Keep a history of copied text (off by default; secrets are skipped). */
  clipboardHistory?: boolean;
  /** The job search tracker (💼). */
  jobsEnabled?: boolean;
  /** Custom assistants (🤖). */
  assistantsEnabled?: boolean;
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
  /** The custom assistant the chat was started with. */
  assistantId?: string | null;
}

/** Rust `assistants::Assistant`: BYTE set up for one job. */
export interface Assistant {
  id: string;
  name: string;
  emoji: string;
  instructions: string;
  starters: string[];
  /** fast, auto, deep, extended, or "" for the current mode. */
  mode: string;
  created: number;
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
  | ({ kind: "macDone" } & MacDone)
  | ({ kind: "automationRun" } & RunCard)
  | ({ kind: "reviews" } & Reviews)
  | ({ kind: "prices" } & Prices)
  | ({ kind: "hints" } & GameHints)
  | ({ kind: "selfCheck" } & SelfCheck)
  | ({ kind: "flashcards" } & Flashcards)
  | ({ kind: "quiz" } & Quiz)
  | ({ kind: "storage" } & Storage)
  | ({ kind: "health" } & Health);

/** A to-do (Rust `tasks::Task`); times are ms since 1970. */
export interface Task {
  id: number;
  title: string;
  notes: string;
  due: number | null;
  remindAt: number | null;
  repeat: "" | "daily" | "weekdays" | "weekly" | "monthly";
  doneAt: number | null;
  created: number;
}

/** Something BYTE does on its own on a schedule (Rust `scheduler::Schedule`). */
export interface Schedule {
  id: number;
  kind: "briefing" | "prompt";
  name: string;
  /** "weekdays 07:30" */
  spec: string;
  prompt: string;
  enabled: boolean;
  lastRun: number | null;
  nextRun: number | null;
  /** "Every weekday at 7:30 AM" */
  when: string;
  lastChat: string | null;
  lastOk: boolean | null;
}

/** A followed news feed (Rust `feeds::Feed`). */
export interface Feed {
  id: number;
  url: string;
  title: string;
  site: string;
  added: number;
  lastChecked: number | null;
  lastError: string;
  /** New items not yet in a digest. */
  unseen: number;
}

/** One step of an automation (Rust `automations::Step`). `{previous}` is the text from the step before. */
export type AutoStep =
  | { type: "ask"; prompt: string }
  | { type: "briefing" }
  | { type: "notify"; title: string; body: string }
  | { type: "addTask"; title: string }
  | { type: "saveFile"; name: string }
  | { type: "shortcut"; name: string };

/** An automation (Rust `automations::Automation`). `trigger`: "manual", "launch" or a schedule spec. */
export interface Automation {
  id: number;
  name: string;
  trigger: string;
  steps: AutoStep[];
  enabled: boolean;
  lastRun: number | null;
  nextRun: number | null;
  when: string;
  lastOk: boolean | null;
  lastChat: string | null;
  /** A Shortcut can start it. */
  linked: boolean;
}

/** How a step of a run went. */
export interface StepRun {
  label: string;
  status: "waiting" | "running" | "done" | "failed";
  detail: string;
}

/** A run's state (`automations://progress`). */
export interface RunView {
  runId: number;
  automationId: number;
  name: string;
  steps: StepRun[];
  finished: boolean;
  ok: boolean;
  chat: string | null;
}

/** The chat card for a run started from chat. */
export interface RunCard {
  runId: number;
  automationId: number;
  name: string;
  when: string;
  steps: StepRun[];
}

/** Making a Shortcut for an automation. */
export interface ShortcutMade {
  opened: boolean;
  link: string;
  message: string;
}

/** Connectors (Rust `connectors::Status`); secrets are never sent to the UI. */
export interface ConnectorsStatus {
  keychain: boolean;
  vault: string | null;
  vaultNotes: number;
  notion: boolean;
  notionParent: string | null;
  /** [name, host]. */
  calendars: [string, string][];
}

export type TrackerKind = "package" | "bill" | "event" | "upkeep";

/** Something BYTE tracks (Rust `trackers::Tracker`). Dates are "YYYY-MM-DD". */
export interface Tracker {
  id: number;
  kind: TrackerKind;
  name: string;
  next: string | null;
  noticeDays: number | null;
  notes: string;
  done: boolean;
  carrier: string;
  number: string;
  amount: number | null;
  currency: string;
  cycle: "" | "weekly" | "monthly" | "quarterly" | "yearly";
  person: string;
  occasion: string;
  ideas: string[];
  budget: number | null;
  everyDays: number | null;
  everyMonths: number | null;
  lastDone: string | null;
  link: string;
  notifiedFor: string | null;
}

/** A watched page (Rust `watchers::Watcher`). */
export interface Watcher {
  id: number;
  url: string;
  name: string;
  kind: "change" | "price";
  /** Price watchers: notify at or below this. */
  target: number | null;
  everyHours: number;
  enabled: boolean;
  created: number;
  lastChecked: number | null;
  nextCheck: number | null;
  lastPrice: number | null;
  currency: string;
  lastChange: number | null;
  /** "Dropped to $179 (was $199)" */
  lastNote: string;
  lastError: string;
}

export interface WatchEvent {
  at: number;
  note: string;
}

/** What's using the disk (Rust `upkeep::Storage`). Ids go back to `upkeep_trash` / `upkeep_reveal`. */
export interface Storage {
  scanId: string;
  total: number;
  free: number;
  folders: { name: string; path: string; bytes: number }[];
  suggestions: { id: string; title: string; why: string; bytes: number; items: string[]; count: number; canTrash: boolean }[];
  big: { id: string; name: string; path: string; bytes: number; daysOld: number | null }[];
  partial: boolean;
}

/** What moving to the Trash did (Rust `upkeep::Trashed`). */
export interface Trashed {
  moved: number;
  bytes: number;
  undo: string | null;
  error: string | null;
}

/** A Mac health check (Rust `upkeep::Health`). */
export interface Health {
  id: string;
  title: string;
  checks: { label: string; value: string; level: "bad" | "warn" | "ok" | "info"; tip: string; settings: string | null; settingsLabel: string | null }[];
  procs: { name: string; cpu: number; mem: string; app: string | null }[];
}

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

/** One copied text in the clipboard history (Rust `clipboard::Clip`). */
export interface Clip {
  id: number;
  text: string;
  /** When it was copied (ms since 1970). */
  at: number;
}

/** Text the ⌥⌘B hotkey read from another app (Rust `selection::Captured`). */
export interface Captured {
  text: string;
  app: string;
}

/** BYTE did something in a Mac app (Rust `macctl::MacDone`). `undo` is a token for `mac_undo`. */
export interface MacDone {
  app: string;
  title: string;
  detail: string;
  ok: boolean;
  undo: string | null;
}

/** The web agent asks before submitting, committing or downloading (Rust `web_agent::ApprovalAsk`). */
export interface ApprovalAsk {
  id: string;
  action: "submit" | "download" | "click" | "mac";
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

/** Rust `settings::ModelOverride`: advanced tuning for one model (null/empty = the model's recommended value). */
export interface ModelOverride {
  /** 0–2. */
  temperature?: number | null;
  /** 0.05–1. */
  topP?: number | null;
  /** Tokens the model may think before answering (-1 = no limit). */
  thinkingBudget?: number | null;
  /** Extra instructions added to every chat with this model. */
  systemExtra?: string;
}

/** Rust `lab::LabModel`: a model added by hand (a GGUF file on this Mac, or from Hugging Face). */
export interface LabModel {
  /** "local-…" or "hf-…"; its catalog key is `${id}:${quant}`. */
  id: string;
  name: string;
  source: "file" | "huggingface";
  /** The file on this Mac (source "file"). */
  path: string;
  /** Hugging Face repo and file (source "huggingface"). */
  repo: string;
  file: string;
  sizeBytes: number;
  /** From the file's header, e.g. "qwen3", "llama", "gemma3". */
  architecture: string;
  /** Billions of parameters (estimated from size and quantization when the header doesn't say). */
  paramsB: number | null;
  /** e.g. "Q4_K_M". */
  quant: string;
  layers: number;
  contextMax: number;
  /** Its chat template can think (<think> / enable_thinking). */
  thinking: boolean;
  /** "great": fits comfortably; "tight": fits, little room left; "no": too big for this Mac. */
  fit: "great" | "tight" | "no";
  /** Plain words, e.g. "Fits comfortably with a 16k context (uses about 6.1 GB)". */
  fitNote: string;
  /** Context size BYTE suggests for it on this Mac. */
  context: number;
  /** Added to BYTE (shows in the model list). */
  added: boolean;
}

/** Rust `commands::LiveStats`: meters for the tuning panel (polled every few seconds). */
export interface LiveStats {
  ramUsedBytes: number;
  ramTotalBytes: number;
  /** Memory the running engine uses (null when none is running). */
  engineRssBytes: number | null;
  /** How much memory the GPU may use on this Mac. */
  gpuBudgetBytes: number;
  battery: { percent: number; charging: boolean } | null;
  /** Battery saver is on and in effect right now. */
  batterySaving: boolean;
  /** The loaded model's recommended sampling (where the sliders sit while "Recommended"). */
  recommended?: { temperature: number; topP: number } | null;
}

/** Rust `jobs::Job`: a job in the tracker. */
export interface Job {
  id: number;
  company: string;
  role: string;
  location: string;
  pay: string;
  url: string;
  /** saved, applied, interview, offer, rejected */
  status: string;
  /** YYYY-MM-DD or empty. */
  deadline: string;
  applied: string;
  summary: string;
  requirements: string[];
  notes: string;
  updated: number;
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
