import { Channel, invoke } from "@tauri-apps/api/core";

import type { LongAsk, Outline } from "./writing";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { DocEvent, DocKind, DocOutline, DocSpec } from "./docs/spec";

import type {
  UpdateInfo,
  MessagesStatus,
  MessageThread,
  TextMessage,
  NewText,
  Activity,
  BackupInfo,
  ActivityKind,
  LockStatus,
  Permission,
  Trashed,
  VoiceStatus,
  SpeakersStatus,
  SpeechVoice,
  VoicePackage, VoicesStatus, CloudVoice, Note, NoteInput, NotesInfo, MapNode, BoardInfo, BoardAssist,
  MediaStatus,
  Task,
  Schedule,
  Feed,
  Watcher,
  Automation,
  Tracker,
  ConnectorsStatus,
  DashboardSummary,
  Usage,
  Researched,
  RunView,
  ShortcutMade,
  WatchEvent,
  BoostInfo,
  Captured,
  Clip,
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
  /** Device, memory, engine and recent log as text to paste into a chat (no chats, no keys). */
  diagnosticsReport: () => invoke<string>("diagnostics_report"),
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
      /** The answer will be heard (BYTE answers like a person talking). */
      spoken?: boolean;
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
  agentApprove: (id: string, ok: boolean, edits?: { label: string; value: string }[]) => invoke<boolean>("agent_approve", { id, ok, edits: edits ?? null }),
  macUndo: (token: string) => invoke<boolean>("mac_undo", { token }),
  // To-dos and schedules (tasks.rs, scheduler.rs).
  tasksList: () => invoke<Task[]>("tasks_list", { includeDone: true }),
  taskSave: (task: Task) => invoke<Task>("task_save", { task }),
  taskDone: (id: number, done: boolean) => invoke<Task>("task_done", { id, done }),
  taskDelete: (id: number) => invoke<void>("task_delete", { id }),
  schedulesList: () => invoke<Schedule[]>("schedules_list"),
  scheduleSave: (schedule: Schedule) => invoke<Schedule>("schedule_save", { schedule }),
  scheduleDelete: (id: number) => invoke<void>("schedule_delete", { id }),
  /** Runs it now; the new chat's id (null: it couldn't run, a notification says why). */
  scheduleRun: (id: number) => invoke<string | null>("schedule_run", { id }),
  /** "every weekday at 8am" → [spec, description], or null. */
  scheduleParse: (text: string) => invoke<[string, string] | null>("schedule_parse", { text }),
  onScheduleRan: (cb: (chatId: string) => void): Promise<UnlistenFn> => listen<string>("schedules://ran", (e) => cb(e.payload)),
  // News feeds and page watchers (feeds.rs, watchers.rs).
  feedsList: () => invoke<Feed[]>("feeds_list"),
  feedFollow: (url: string) => invoke<Feed>("feed_follow", { url }),
  feedDelete: (id: number) => invoke<void>("feed_delete", { id }),
  watchersList: () => invoke<Watcher[]>("watchers_list"),
  watcherSave: (watcher: Watcher) => invoke<Watcher>("watcher_save", { watcher }),
  watcherDelete: (id: number) => invoke<void>("watcher_delete", { id }),
  watcherEvents: (id: number) => invoke<WatchEvent[]>("watcher_events", { id }),
  watcherCheck: (id: number) => invoke<Watcher>("watcher_check", { id }),
  onWatcherChanged: (cb: (id: number) => void): Promise<UnlistenFn> => listen<number>("watchers://changed", (e) => cb(e.payload)),
  // Automations (automations.rs, shortcut_make.rs).
  automationsList: () => invoke<Automation[]>("automations_list"),
  automationSave: (automation: Automation) => invoke<Automation>("automation_save", { automation }),
  automationDelete: (id: number) => invoke<void>("automation_delete", { id }),
  /** Runs it now (from step `from`, 0-based); the run's id. */
  automationRun: (id: number, from?: number) => invoke<number>("automation_run", { id, from: from ?? null }),
  automationRunStatus: (runId: number) => invoke<RunView | null>("automation_run_status", { runId }),
  /** "every weekday at 8am" / "when BYTE opens" → [trigger, description], or null. */
  automationTriggerParse: (text: string) => invoke<[string, string] | null>("automation_trigger_parse", { text }),
  automationShortcut: (id: number) => invoke<ShortcutMade>("automation_shortcut", { id }),
  onAutomationProgress: (cb: (v: RunView) => void): Promise<UnlistenFn> => listen<RunView>("automations://progress", (e) => cb(e.payload)),
  /** A byte://ask link: text for the message box (never sent by itself). */
  onDeepLinkAsk: (cb: (text: string) => void): Promise<UnlistenFn> => listen<string>("deeplink://ask", (e) => cb(e.payload)),
  // Quick Ask and the menu-bar icon (quick.rs).
  quickToggle: () => invoke<void>("quick_toggle"),
  quickHide: () => invoke<void>("quick_hide"),
  quickOpen: (conversationId: string | null) => invoke<void>("quick_open", { conversationId }),
  /** Quick Ask was shown (focus the box). */
  onQuickShown: (cb: () => void): Promise<UnlistenFn> => listen("quick://shown", () => cb()),
  /** Main window: "Open in BYTE" from Quick Ask. */
  onQuickOpen: (cb: (id: string | null) => void): Promise<UnlistenFn> => listen<string | null>("quick://open", (e) => cb(e.payload)),
  /** Main window: Quick Ask saved a chat. */
  onQuickSaved: (cb: () => void): Promise<UnlistenFn> => listen("quick://saved", () => cb()),
  // Voice input (voice.rs).
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  voiceDownload: (id: string) => invoke<void>("voice_download", { id }),
  voiceDelete: (id: string) => invoke<void>("voice_delete", { id }),
  voiceTranscribe: (wavBase64: string) => invoke<string>("voice_transcribe", { wavBase64 }),
  // Spoken replies (speech.rs) and "Hey BYTE" (wake.rs).
  speechSay: (text: string, voice?: string) => invoke<void>("speech_say", { text, voice: voice ?? null }),
  speechStop: () => invoke<void>("speech_stop"),
  /** More of answer `id` (all its text so far) for BYTE's voices to read; `done` when it's complete. */
  speechFeed: (id: string, text: string, done: boolean, isPrivate = false) => invoke<void>("speech_feed", { id, text, done, private: isPrivate }),
  // BYTE's own voices (tts.rs).
  // Notes (notes.rs) and mind maps (mindmap.rs).
  notesList: (query?: string) => invoke<Note[]>("notes_list", { query: query ?? null }),
  noteGet: (id: string) => invoke<Note>("note_get", { id }),
  noteSave: (note: NoteInput) => invoke<Note>("note_save", { note }),
  noteDelete: (id: string) => invoke<void>("note_delete", { id }),
  notesInfo: () => invoke<NotesInfo>("notes_info"),
  noteClip: (url: string, selection?: string) => invoke<Note>("note_clip", { url, selection: selection ?? null }),
  onNoteClipped: (cb: (id: string) => void): Promise<UnlistenFn> => listen<string>("notes://clipped", (e) => cb(e.payload)),
  boardsList: () => invoke<BoardInfo[]>("boards_list"),
  boardGet: (id: number) => invoke<{ id: number; title: string; data: unknown; updated: number }>("board_get", { id }),
  boardSave: (id: number, title: string, data: unknown) => invoke<number>("board_save", { id, title, data }),
  boardDelete: (id: number) => invoke<void>("board_delete", { id }),
  boardAssist: (kind: "ideas" | "expand" | "group", topic: string, stickies: string[], focus?: string) =>
    invoke<BoardAssist>("board_assist", { kind, topic, stickies, focus: focus ?? null }),
  mindmapMake: (text: string, title: string) => invoke<MapNode>("mindmap_make", { text, title }),
  voicesCatalog: () => invoke<VoicePackage[]>("voices_catalog"),
  voicesStatus: () => invoke<VoicesStatus>("voices_status"),
  ttsVoiceDownload: (id: string) => invoke<void>("tts_voice_download", { id }),
  ttsVoiceUnpack: (id: string) => invoke<void>("tts_voice_unpack", { id }),
  ttsVoiceDelete: (id: string) => invoke<void>("tts_voice_delete", { id }),
  cloudVoices: () => invoke<CloudVoice[]>("cloud_voices"),
  speechVoices: () => invoke<SpeechVoice[]>("speech_voices"),
  onSpeechDone: (cb: () => void): Promise<UnlistenFn> => listen("speech://done", () => cb()),
  wakePause: (paused: boolean) => invoke<void>("wake_pause", { paused }),
  wakeReady: () => invoke<boolean>("wake_ready"),
  /** Quick Ask: "Hey BYTE" was heard. */
  onWakeHeard: (cb: () => void): Promise<UnlistenFn> => listen("wake://heard", () => cb()),
  // Speaker labels (speakers.rs) and the video helper (media.rs).
  speakersStatus: () => invoke<SpeakersStatus>("speakers_status"),
  speakersDownload: () => invoke<void>("speakers_download"),
  speakersDelete: () => invoke<void>("speakers_delete"),
  mediaStatus: () => invoke<MediaStatus>("media_status"),
  mediaDownload: () => invoke<void>("media_download"),
  mediaDelete: () => invoke<void>("media_delete"),
  // Trackers (trackers.rs).
  trackersList: () => invoke<Tracker[]>("trackers_list"),
  trackerSave: (tracker: Tracker) => invoke<Tracker>("tracker_save", { tracker }),
  trackerDone: (id: number) => invoke<Tracker>("tracker_done", { id }),
  trackerDelete: (id: number) => invoke<void>("tracker_delete", { id }),
  /** "March 3", "the 12th", "friday" → "YYYY-MM-DD", or null. */
  trackerDateParse: (text: string) => invoke<string | null>("tracker_date_parse", { text }),
  /** A tracking number → [carrier, tracking page], or null. */
  trackerCarrier: (number: string) => invoke<[string, string] | null>("tracker_carrier", { number }),
  // Connectors (connectors/): secrets go to the Keychain in Rust and never come back.
  connectorsStatus: () => invoke<ConnectorsStatus>("connectors_status"),
  obsidianSet: (path: string | null) => invoke<ConnectorsStatus>("obsidian_set", { path }),
  notionConnect: (secret: string, parent: string | null) => invoke<ConnectorsStatus>("notion_connect", { secret, parent }),
  notionDisconnect: () => invoke<ConnectorsStatus>("notion_disconnect"),
  /** Adds a calendar link; [status, events read]. */
  calendarLinkAdd: (name: string, url: string) => invoke<[ConnectorsStatus, number]>("calendar_link_add", { name, url }),
  calendarLinkRemove: (index: number) => invoke<ConnectorsStatus>("calendar_link_remove", { index }),
  // Dashboards (dashboard.rs).
  dashboardSummary: () => invoke<DashboardSummary>("dashboard_summary"),
  /** Today's events: [time, title]. */
  dashboardToday: () => invoke<[string, string][]>("dashboard_today"),
  dashboardUsage: () => invoke<Usage>("dashboard_usage"),
  researchLibrary: (query?: string) => invoke<Researched[]>("research_library", { query: query ?? null }),
  // Mac upkeep (upkeep.rs): ids from the card, never paths.
  upkeepTrash: (scanId: string, id: string) => invoke<Trashed>("upkeep_trash", { scanId, id }),
  upkeepReveal: (scanId: string, id: string) => invoke<void>("upkeep_reveal", { scanId, id }),
  upkeepQuit: (card: string, app: string) => invoke<boolean>("upkeep_quit", { card, app }),
  upkeepOpenSettings: (url: string) => invoke<void>("upkeep_open_settings", { url }),
  // The lock (lock.rs) and Settings → Privacy (privacy.rs, offline.rs).
  lockStatus: () => invoke<LockStatus>("lock_status"),
  lockTouch: () => invoke<void>("lock_touch"),
  lockNow: () => invoke<void>("lock_now"),
  lockUnlock: () => invoke<void>("lock_unlock"),
  lockVerify: () => invoke<void>("lock_verify"),
  onLockChanged: (cb: (locked: boolean) => void): Promise<UnlistenFn> => listen<boolean>("lock://changed", (e) => cb(e.payload)),
  onOfflineChanged: (cb: (offline: boolean) => void): Promise<UnlistenFn> => listen<boolean>("privacy://offline", (e) => cb(e.payload)),
  onOpenSettings: (cb: (tab: string) => void): Promise<UnlistenFn> => listen<string>("settings://open", (e) => cb(e.payload)),
  privacyPermissions: () => invoke<Permission[]>("privacy_permissions"),
  actionsList: (query?: string, kind?: ActivityKind, limit?: number) => invoke<Activity[]>("actions_list", { query: query ?? null, kind: kind ?? null, limit: limit ?? null }),
  actionsClear: () => invoke<void>("actions_clear"),
  // Kids mode (kids.rs) and backups (backup.rs).
  kidsEnter: (pin: string) => invoke<Settings>("kids_enter", { pin }),
  kidsExit: (pin: string) => invoke<Settings>("kids_exit", { pin }),
  backupInfo: () => invoke<BackupInfo>("backup_info"),
  backupNow: (passphrase: string | null, remember: boolean) => invoke<string>("backup_now", { passphrase, remember }),
  backupForget: () => invoke<void>("backup_forget"),
  backupRestore: (path: string, passphrase: string) => invoke<void>("backup_restore", { path, passphrase }),
  eraseEverything: (confirm: string) => invoke<void>("erase_everything", { confirm }),
  // One-click updates (updater.rs).
  updateConfigured: () => invoke<boolean>("update_configured"),
  // The Messages inbox (messages.rs).
  messagesStatus: () => invoke<MessagesStatus>("messages_status"),
  notificationsStatus: () => invoke<string>("notifications_status"),
  notificationsRequest: () => invoke<string>("notifications_request"),
  messagesRequestAccess: () => invoke<MessagesStatus>("messages_request_access"),
  messagesThreads: () => invoke<MessageThread[]>("messages_threads"),
  messagesThread: (chat: string) => invoke<TextMessage[]>("messages_thread", { chat }),
  messagesSend: (chat: string, to: string, text: string) => invoke<string>("messages_send", { chat, to, text }),
  onNewTexts: (cb: (t: NewText[]) => void): Promise<UnlistenFn> => listen<NewText[]>("messages://new", (e) => cb(e.payload)),
  updateCheck: () => invoke<UpdateInfo | null>("update_check"),
  updateInstall: () => invoke<void>("update_install"),
  onUpdateAvailable: (cb: (u: UpdateInfo) => void): Promise<UnlistenFn> => listen<UpdateInfo>("update://available", (e) => cb(e.payload)),
  onUpdateProgress: (cb: (p: { got: number; total: number | null }) => void): Promise<UnlistenFn> => listen<{ got: number; total: number | null }>("update://progress", (e) => cb(e.payload)),
  selectionPaste: (appName: string, text: string) => invoke<void>("selection_paste", { appName, text }),
  onSelection: (cb: (c: Captured) => void): Promise<UnlistenFn> => listen<Captured>("selection://captured", (e) => cb(e.payload)),
  onSelectionError: (cb: (message: string) => void): Promise<UnlistenFn> => listen<string>("selection://error", (e) => cb(e.payload)),
  clipList: (query?: string) => invoke<Clip[]>("clip_list", { query: query ?? null }),
  clipCopy: (id: number) => invoke<void>("clip_copy", { id }),
  clipDelete: (id: number) => invoke<void>("clip_delete", { id }),
  clipClear: () => invoke<void>("clip_clear"),
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
