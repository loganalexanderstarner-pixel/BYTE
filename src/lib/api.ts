import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  ChatEvent,
  DownloadEvent,
  EngineStatus,
  Mode,
  ModelStatus,
  Settings,
  SystemInfo,
  ThinkingPref,
  WireMessage,
} from "./types";

/** True when running inside the Tauri shell (false in a plain browser tab). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api = {
  systemInfo: () => invoke<SystemInfo>("system_info"),
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsUpdate: (patch: Partial<Settings>) => invoke<Settings>("settings_update", { patch }),

  modelsList: () => invoke<ModelStatus[]>("models_list"),
  modelDownload: (id: string) => invoke<void>("model_download", { id }),
  modelPause: (id: string) => invoke<void>("model_pause", { id }),
  modelDelete: (id: string) => invoke<void>("model_delete", { id }),
  modelActivate: (id: string) => invoke<void>("model_activate", { id }),

  engineStatus: () => invoke<EngineStatus>("engine_status"),
  engineRestart: () => invoke<void>("engine_restart"),
  engineLog: () => invoke<string[]>("engine_log"),

  chatSend: (
    request: { requestId: string; messages: WireMessage[]; mode: Mode; thinking: ThinkingPref },
    onEvent: (e: ChatEvent) => void,
  ) => {
    const channel = new Channel<ChatEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("chat_send", { request, onEvent: channel });
  },
  chatCancel: (requestId: string) => invoke<boolean>("chat_cancel", { requestId }),
};

export const events = {
  onEngineStatus: (cb: (s: EngineStatus) => void): Promise<UnlistenFn> =>
    listen<EngineStatus>("engine://status", (e) => cb(e.payload)),
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
