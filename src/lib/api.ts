import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  BoostInfo,
  GpuShare,
  ChatSummary,
  TuneProgress,
  Tuning,
  ConversationMeta,
  Profile,
  Profiles,
  Project,
  LoadedModel,
  Memory,
  SearchHit,
  ChatEvent,
  DownloadEvent,
  EngineStatus,
  Mode,
  ModelStatus,
  Settings,
  SystemInfo,
  ThinkingPref,
  WireMessage,
  CloudStatus,
} from "./types";

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
}

export type CloudAction = "regenerate" | "deepen" | "justify" | "stop" | "answer-now" | "feedback";

export const api = {
  systemInfo: () => invoke<SystemInfo>("system_info"),
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
      cloud?: CloudTurn;
    },
    onEvent: (e: ChatEvent) => void,
  ) => {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("chat_send", { request, onEvent: channel });
  },
  chatCancel: (requestId: string) => invoke<boolean>("chat_cancel", { requestId }),

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
  cloudDeleteMessage: (messageId: string) => invoke<void>("cloud_delete_message", { messageId }),
  cloudConversations: () => invoke<unknown>("cloud_conversations"),
  cloudImport: (conversationId: string) => invoke<string>("cloud_import", { conversationId }),

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
