import { Channel, invoke } from "@tauri-apps/api/core";

import type { LongAsk, Outline } from "./writing";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { DocEvent, DocKind, DocOutline, DocSpec } from "./docs/spec";

import type {
  BoostInfo,
  ChatEvent,
  ChatSummary,
  CloudStatus,
  ConversationMeta,
  DownloadEvent,
  EngineStatus,
  GpuShare,
  LoadedModel,
  Memory,
  MemoryReport,
  KbHit,
  KbProgress,
  KbStatus,
  LookerStatus,
  Job,
  Assistant,
  LabModel,
  LiveStats,
  LocalFile,
  Mode,
  ModelStatus,
  Profile,
  Profiles,
  Project,
  SearchHit,
  Settings,
  Source,
  SystemInfo,
  ThinkingPref,
  TuneProgress,
  Tuning,
  WireMessage,
  ChatTask,
  Recipe,
  SavedRecipe, DeckSummary, StudyCard } from "./types";

/** True when running inside the Tauri shell (false in a plain browser tab). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Sidebar changes to a chat; "" clears folder/project. */
export interface ChatPatch {
  title?: string;
  pinned?: boolean;
  folder?: string;
  projectId?: string;
}

/** The cloud part of a chat request. */
export interface CloudTurn {
  conversationId?: string | null;
  mode: string;
  lastRemoteId?: string | null;
  attachmentIds?: (string | number)[];
  branchFrom?: string | null;
  /** Both workspace: report an unreachable cloud instead of answering on this Mac a second time. */
  noFallback?: boolean;
}

export type CloudAction = "regenerate" | "deepen" | "justify" | "stop" | "answer-now" | "feedback";

export const api = {
  systemInfo: () => invoke<SystemInfo>("system_info"),
  memoryReport: () => invoke<MemoryReport>("memory_report"),
  appQuit: (name: string) => invoke<void>("app_quit", { name }),
  fileIngest: (path: string) => invoke<LocalFile>("file_ingest", { path }),
  kbStatus: () => invoke<KbStatus>("kb_status"),
  lookerStatus: () => invoke<LookerStatus>("looker_status"),
  // Model lab: add any GGUF (a file on this Mac or a Hugging Face link)
  /** Reads a GGUF file's header and checks it against this Mac's memory (nothing is added yet). */
  labInspect: (path: string) => invoke<LabModel>("lab_inspect", { path }),
  /** Same for a Hugging Face file link (reads only the header, not the whole file). */
  labInspectUrl: (url: string) => invoke<LabModel>("lab_inspect_url", { url }),
  /** Adds it to the model list; returns its catalog key. Hugging Face models are then downloaded with `modelDownload(key)`. */
  labAdd: (model: LabModel) => invoke<string>("lab_add", { model }),
  labList: () => invoke<LabModel[]>("lab_list"),
  labRemove: (id: string) => invoke<void>("lab_remove", { id }),
  /** Memory, engine and battery meters. */
  engineLive: () => invoke<LiveStats>("engine_live"),
  jobsList: () => invoke<Job[]>("jobs_list"),
  assistantsList: () => invoke<Assistant[]>("assistants_list"),
  assistantPresets: () => invoke<Assistant[]>("assistant_presets"),
  assistantSave: (assistant: Assistant) => invoke<string>("assistant_save", { assistant }),
  assistantDelete: (id: string) => invoke<void>("assistant_delete", { id }),
  jobSave: (job: Job) => invoke<number>("job_save", { job }),
  jobDelete: (id: number) => invoke<void>("job_delete", { id }),
  jobFromUrl: (url: string) => invoke<Job>("job_from_url", { url }),
  jobPrepPrompt: (job: Job) => invoke<string>("job_prep_prompt", { job }),
  kbAdd: (path: string) => invoke<number>("kb_add", { path }),
  kbRemove: (id: number) => invoke<void>("kb_remove", { id }),
  /** Re-reads changed files in one folder, or all when `id` is omitted. */
  kbReindex: (id?: number) => invoke<void>("kb_reindex", { id: id ?? null }),
  kbSearch: (query: string, limit?: number) => invoke<KbHit[]>("kb_search", { query, limit: limit ?? null }),
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsUpdate: (patch: Partial<Settings>) => invoke<Settings>("settings_update", { patch }),

  modelsList: () => invoke<ModelStatus[]>("models_list"),
  modelRecommend: () => invoke<string | null>("model_recommend"),
  catalogRefresh: () => invoke<boolean>("catalog_refresh"),
  modelDownload: (key: string) => invoke<void>("model_download", { key }),
  modelPause: (key: string) => invoke<void>("model_pause", { key }),
  modelDelete: (key: string) => invoke<void>("model_delete", { key }),
  modelActivate: (key: string) => invoke<void>("model_activate", { key }),
  modelsLoaded: () => invoke<LoadedModel[]>("models_loaded"),
  /** Loads a model alongside the main one (both stay in memory). */
  modelLoad: (key: string) => invoke<void>("model_load", { key }),
  modelUnload: (key: string) => invoke<boolean>("model_unload", { key }),

  engineStatus: () => invoke<EngineStatus>("engine_status"),
  engineRestart: () => invoke<void>("engine_restart"),
  engineLog: () => invoke<string[]>("engine_log"),
  speedBoostInfo: () => invoke<BoostInfo>("speed_boost_info"),
  gpuShareInfo: () => invoke<GpuShare>("gpu_share_info"),
  gpuShareSet: (raise: boolean) => invoke<GpuShare>("gpu_share_set", { raise }),
  /** Measures and keeps the fastest engine settings for the active model (quick 1–2 min, thorough ~5 min). */
  engineTune: (thorough = false) => invoke<Tuning>("engine_tune", { thorough }),
  /** Tunes every downloaded model that fits, then returns to the one in use. Returns how many. */
  engineTuneAll: (thorough = false) => invoke<number>("engine_tune_all", { thorough }),

  chatSend: (
    request: {
      requestId: string;
      messages: WireMessage[];
      mode: Mode;
      thinking: ThinkingPref;
      model?: string;
      private?: boolean;
      projectId?: string | null;
      assistantId?: string | null;
      cloud?: CloudTurn;
      /** Don't reuse an earlier answer (Regenerate). */
      fresh?: boolean;
      /** A job asked for with a button (Rust `agent::Task`). */
      task?: ChatTask;
    },
    onEvent: (e: ChatEvent) => void,
  ) => {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("chat_send", { request, onEvent: channel });
  },
  chatCancel: (requestId: string) => invoke<boolean>("chat_cancel", { requestId }),
  /** Remembers a chat's first answer for instant reuse (Rust decides whether it qualifies). */
  answerCachePut: (question: string, mode: Mode, answer: string, sources: Source[]) =>
    invoke<void>("answer_cache_put", { question, mode, answer, sources }),
  answerCacheClear: () => invoke<void>("answer_cache_clear"),
  // Documents made on this Mac (docs.rs); files are rendered in lib/docs.
  docOutline: (kind: DocKind, prompt: string, referencePath: string | null) => invoke<DocOutline>("doc_outline", { kind, prompt, referencePath }),
  docWrite: (
    request: { requestId: string; kind: DocKind; prompt: string; outline: DocOutline; research: boolean; referencePath: string | null },
    onEvent: (e: DocEvent) => void,
  ) => {
    const channel = new Channel<DocEvent>();
    channel.onmessage = onEvent;
    return invoke<DocSpec>("doc_write", { request, onEvent: channel });
  },
  docSave: (path: string, data: string) => invoke<void>("doc_save", { path, data }),
  recipesList: (query?: string) => invoke<SavedRecipe[]>("recipes_list", { query: query ?? null }),
  recipeSave: (recipe: Recipe) => invoke<number>("recipe_save", { recipe }),
  recipeDelete: (id: number) => invoke<void>("recipe_delete", { id }),
  /** Saves an .ics file and opens it in the calendar app. */
  calendarOpen: (path: string, data: string) => invoke<void>("calendar_open", { path, data }),

  // Study decks (study.rs).
  decksList: () => invoke<DeckSummary[]>("decks_list"),
  deckSave: (name: string, cards: { front: string; back: string }[]) => invoke<number>("deck_save", { name, cards }),
  deckCards: (id: number) => invoke<StudyCard[]>("deck_cards", { id }),
  studyQueue: (id: number) => invoke<StudyCard[]>("study_queue", { id }),
  cardReview: (id: number, grade: number) => invoke<StudyCard>("card_review", { id, grade }),
  deckDelete: (id: number) => invoke<void>("deck_delete", { id }),
  cardDelete: (id: number) => invoke<void>("card_delete", { id }),
  /** The deck as an Anki import file (text). */
  deckExport: (id: number) => invoke<string>("deck_export", { id }),

  // Web agent (web_agent/ in Rust).
  /** Answers an approval card; false when it's no longer waiting. */
  agentApprove: (id: string, ok: boolean) => invoke<boolean>("agent_approve", { id, ok }),
  /** Shows or hides the agent's browser window; false when none is open. */
  agentShow: (visible: boolean) => invoke<boolean>("agent_show", { visible }),
  /** Opens a saved page/picture, or shows any saved file in Finder. */
  agentFile: (path: string, open: boolean) => invoke<void>("agent_file", { path, open }),

  // BYTE cloud (docs/CLOUD-MODE.md). The key goes straight to the Keychain.
  cloudStatus: () => invoke<CloudStatus>("cloud_status"),
  cloudConnect: (key: string, baseUrl?: string | null) => invoke<CloudStatus>("cloud_connect", { key, baseUrl: baseUrl ?? null }),
  cloudDisconnect: () => invoke<CloudStatus>("cloud_disconnect"),
  cloudRefresh: () => invoke<CloudStatus>("cloud_refresh"),
  cloudAction: (
    request: { requestId: string; conversationId: string; messageId: string; action: CloudAction; since?: string | null; value?: string },
    onEvent: (e: ChatEvent) => void,
  ) => {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("cloud_action", { request, onEvent: channel });
  },
  /** Writing studio: the new text streams as `content` events. */
  writingRun: (requestId: string, text: string, action: string, tone: string | null, likeMe: boolean, onEvent: (e: ChatEvent) => void) => {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("writing_run", { requestId, text, action, tone, likeMe, onEvent: channel });
  },
  writingOutline: (ask: LongAsk) => invoke<Outline>("writing_outline", { ask }),
  /** One part of a long piece (or the whole poem); streams `content`. */
  writingSection: (requestId: string, ask: LongAsk, outline: Outline, index: number, before: string, onEvent: (e: ChatEvent) => void) => {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("writing_section", { requestId, ask, outline, index, before, onEvent: channel });
  },
  /** Learns and saves a style profile from samples of the user's writing. */
  styleLearn: (samples: string[]) => invoke<string>("style_learn", { samples }),
  cloudDeleteMessage: (messageId: string) => invoke<void>("cloud_delete_message", { messageId }),
  cloudConversations: () => invoke<unknown>("cloud_conversations"),
  cloudImport: (conversationId: string) => invoke<string>("cloud_import", { conversationId }),
  /** Any `/api/…` call on the cloud (documents, library, memories, …). */
  cloudGet: <T = unknown>(path: string) => invoke<T>("cloud_get", { path }),
  cloudPost: <T = unknown>(path: string, body?: unknown) => invoke<T>("cloud_post", { path, body: body ?? null }),
  cloudDelete: (path: string) => invoke<unknown>("cloud_delete", { path }),
  /** A cloud image as a data: URL. */
  cloudImage: (path: string) => invoke<string>("cloud_image", { path }),
  /** Saves a cloud file to `dest`; returns its size. */
  cloudDownload: (path: string, dest: string) => invoke<number>("cloud_download", { path, dest }),
  cloudUpload: <T = unknown>(path: string, file: string) => invoke<T>("cloud_upload", { path, file }),
  /** Uploads a photo/file for a chat, starting its cloud conversation if needed. */
  cloudAttach: (conversationId: string | null, title: string, file: string) =>
    invoke<{ conversationId: string; attachment: Record<string, unknown> }>("cloud_attach", { conversationId, title, file }),

  // Saved chats (encrypted database)
  chatsList: () => invoke<ConversationMeta[]>("chats_list"),
  chatLoad: (id: string) => invoke<unknown | null>("chat_load", { id }),
  chatSave: (conversation: unknown) => invoke<void>("chat_save", { conversation }),
  chatDelete: (id: string) => invoke<void>("chat_delete", { id }),
  chatUpdate: (id: string, patch: ChatPatch) => invoke<void>("chat_update", { id, patch }),
  /** BYTE writes a title, one-line summary and tags for a chat (once). */
  chatAutotitle: (id: string) => invoke<ChatSummary | null>("chat_autotitle", { id }),

  // Projects
  projectsList: () => invoke<Project[]>("projects_list"),
  projectSave: (project: Project) => invoke<Project>("project_save", { project }),
  projectDelete: (id: string) => invoke<void>("project_delete", { id }),

  // Profiles (switching restarts BYTE)
  profilesList: () => invoke<Profiles>("profiles_list"),
  profileCreate: (name: string) => invoke<Profile>("profile_create", { name }),
  profileRename: (id: string, name: string) => invoke<void>("profile_rename", { id, name }),
  profileDelete: (id: string) => invoke<void>("profile_delete", { id }),
  profileSwitch: (id: string) => invoke<void>("profile_switch", { id }),
  chatsSearch: (query: string) => invoke<SearchHit[]>("chats_search", { query }),
  chatsImport: (conversations: unknown[]) => invoke<number>("chats_import", { conversations }),
  /** Exports every chat into a new folder inside `dir`; returns its path. */
  chatsExport: (dir: string) => invoke<string>("chats_export", { dir }),

  // Memory
  memoriesList: () => invoke<Memory[]>("memories_list"),
  memoryAdd: (text: string, source: "user" | "chat" = "user") => invoke<Memory>("memory_add", { text, source }),
  memoryUpdate: (id: string, text: string) => invoke<void>("memory_update", { id, text }),
  memoryDelete: (id: string) => invoke<void>("memory_delete", { id }),
  /** Erases every saved chat and memory. */
  dataWipe: () => invoke<void>("data_wipe"),
};

export const events = {
  onEngineStatus: (cb: (s: EngineStatus) => void): Promise<UnlistenFn> =>
    listen<EngineStatus>("engine://status", (e) => cb(e.payload)),
  onTune: (cb: (p: TuneProgress) => void): Promise<UnlistenFn> => listen<TuneProgress>("engine://tune", (e) => cb(e.payload)),
  /** A model loaded alongside the main one changed state. */
  onExtras: (cb: () => void): Promise<UnlistenFn> => listen("engine://extras", () => cb()),
  onDownload: (cb: (e: DownloadEvent) => void): Promise<UnlistenFn> =>
    listen<DownloadEvent>("models://download", (e) => cb(e.payload)),
  onKbProgress: (cb: (p: KbProgress) => void): Promise<UnlistenFn> => listen<KbProgress>("kb://progress", (e) => cb(e.payload)),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}
