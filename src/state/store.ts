import { create } from "zustand";

import { api, errorText, events, inTauri } from "../lib/api";
import { titleFrom } from "../lib/format";
import type {
  ChatEvent,
  DownloadEvent,
  EngineStatus,
  Mode,
  ModelStatus,
  Settings,
  Stats,
  SystemInfo,
  ThinkingPref,
  WireMessage,
} from "../lib/types";

export interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  reasoning?: string;
  thinking?: boolean;
  status: "streaming" | "done" | "error" | "cancelled";
  error?: string;
  stats?: Stats;
  mode?: Mode;
  model?: string;
  createdAt: number;
}

export interface Conversation {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: Message[];
}

export interface DownloadState {
  phase: "resuming" | "downloading" | "verifying" | "paused" | "failed" | "finished";
  bytes: number;
  total: number;
  bytesPerSec: number;
  error?: string;
}

export type SettingsTab = "models" | "appearance" | "engine" | "about";

interface State {
  ready: boolean;
  settings: Settings | null;
  system: SystemInfo | null;
  models: ModelStatus[];
  engine: EngineStatus;
  downloads: Record<string, DownloadState>;
  conversations: Conversation[];
  currentId: string | null;
  generating: string | null;
  mode: Mode;
  thinking: ThinkingPref;
  sidebarOpen: boolean;
  settingsTab: SettingsTab | null;

  init(): Promise<void>;
  updateSettings(patch: Partial<Settings>): Promise<void>;
  refreshModels(): Promise<void>;
  newChat(): void;
  selectChat(id: string): void;
  deleteChat(id: string): void;
  send(text: string): Promise<void>;
  regenerate(): Promise<void>;
  stop(): Promise<void>;
  setMode(m: Mode): void;
  setThinking(t: ThinkingPref): void;
  toggleSidebar(): void;
  openSettings(tab: SettingsTab | null): void;
}

const STORAGE_KEY = "byte.conversations.v1";

function loadConversations(): Conversation[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const list = JSON.parse(raw) as Conversation[];
    // Anything still "streaming" was interrupted by a quit.
    return list.map((c) => ({
      ...c,
      messages: c.messages.map((m) => (m.status === "streaming" ? { ...m, status: "cancelled" as const } : m)),
    }));
  } catch {
    return [];
  }
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;
function saveConversations(list: Conversation[]) {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(list.filter((c) => c.messages.length > 0)));
    } catch {
      /* storage full or unavailable: chats stay in memory */
    }
  }, 400);
}

const uid = () => crypto.randomUUID();

/** Messages sent to the model: finished turns only, in order. */
export function toWire(messages: Message[]): WireMessage[] {
  return messages
    .filter((m) => m.role === "user" || (m.status === "done" && m.content.trim().length > 0))
    .map((m) => ({ role: m.role, content: m.content }));
}

export const useStore = create<State>((set, get) => {
  const patchConversation = (id: string, fn: (c: Conversation) => Conversation) => {
    const conversations = get().conversations.map((c) => (c.id === id ? fn(c) : c));
    set({ conversations });
    saveConversations(conversations);
  };

  const patchMessage = (convId: string, msgId: string, fn: (m: Message) => Message) =>
    patchConversation(convId, (c) => ({
      ...c,
      updatedAt: Date.now(),
      messages: c.messages.map((m) => (m.id === msgId ? fn(m) : m)),
    }));

  /** Streams an assistant reply for the conversation's current history. */
  const generate = async (convId: string) => {
    const conv = get().conversations.find((c) => c.id === convId);
    if (!conv) return;
    const { mode, thinking } = get();
    const history = toWire(conv.messages);
    const reply: Message = { id: uid(), role: "assistant", content: "", reasoning: "", status: "streaming", mode, createdAt: Date.now() };
    patchConversation(convId, (c) => ({ ...c, messages: [...c.messages, reply] }));
    set({ generating: reply.id });

    // Batch token deltas into one render per animation frame.
    let pendingContent = "";
    let pendingReasoning = "";
    let frame = 0;
    const flush = () => {
      frame = 0;
      if (!pendingContent && !pendingReasoning) return;
      const c = pendingContent;
      const r = pendingReasoning;
      pendingContent = "";
      pendingReasoning = "";
      patchMessage(convId, reply.id, (m) => ({ ...m, content: m.content + c, reasoning: (m.reasoning ?? "") + r }));
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(flush);
    };

    const onEvent = (e: ChatEvent) => {
      switch (e.kind) {
        case "started":
          patchMessage(convId, reply.id, (m) => ({ ...m, thinking: e.thinking, model: e.model }));
          break;
        case "reasoning":
          pendingReasoning += e.delta;
          schedule();
          break;
        case "content":
          pendingContent += e.delta;
          schedule();
          break;
        case "stats": {
          const { kind: _kind, ...stats } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, stats }));
          break;
        }
        case "done":
          cancelAnimationFrame(frame);
          flush();
          patchMessage(convId, reply.id, (m) => ({
            ...m,
            status: e.finishReason === "cancelled" ? "cancelled" : "done",
          }));
          break;
      }
    };

    try {
      await api.chatSend({ requestId: reply.id, messages: history, mode, thinking }, onEvent);
    } catch (err) {
      cancelAnimationFrame(frame);
      flush();
      patchMessage(convId, reply.id, (m) => ({ ...m, status: "error", error: errorText(err) }));
    } finally {
      if (get().generating === reply.id) set({ generating: null });
    }
  };

  return {
    ready: false,
    settings: null,
    system: null,
    models: [],
    engine: { state: "stopped" },
    downloads: {},
    conversations: [],
    currentId: null,
    generating: null,
    mode: "auto",
    thinking: "auto",
    sidebarOpen: true,
    settingsTab: null,

    async init() {
      if (get().ready) return;
      const conversations = loadConversations();
      if (!inTauri) {
        set({ ready: true, conversations });
        return;
      }
      await events.onEngineStatus((engine) => {
        set({ engine });
        if (engine.state === "ready" || engine.state === "noModel") void get().refreshModels();
      });
      await events.onDownload((e) => handleDownload(e));
      const [settings, system, models, engine] = await Promise.all([
        api.settingsGet(),
        api.systemInfo(),
        api.modelsList(),
        api.engineStatus(),
      ]);
      set({
        ready: true,
        settings,
        system,
        models,
        engine,
        conversations,
        currentId: conversations[0]?.id ?? null,
        mode: settings.defaultMode,
        thinking: settings.thinking,
      });
    },

    async updateSettings(patch) {
      const settings = await api.settingsUpdate(patch);
      set({ settings });
    },

    async refreshModels() {
      if (!inTauri) return;
      set({ models: await api.modelsList() });
    },

    newChat() {
      const current = get().conversations.find((c) => c.id === get().currentId);
      if (current && current.messages.length === 0) return;
      const conv: Conversation = { id: uid(), title: "New chat", createdAt: Date.now(), updatedAt: Date.now(), messages: [] };
      set({ conversations: [conv, ...get().conversations], currentId: conv.id });
    },

    selectChat(id) {
      set({ currentId: id });
    },

    deleteChat(id) {
      const conversations = get().conversations.filter((c) => c.id !== id);
      set({ conversations, currentId: get().currentId === id ? (conversations[0]?.id ?? null) : get().currentId });
      saveConversations(conversations);
    },

    async send(text) {
      const content = text.trim();
      if (!content || get().generating) return;
      let convId = get().currentId;
      if (!convId || !get().conversations.some((c) => c.id === convId)) {
        const conv: Conversation = { id: uid(), title: "New chat", createdAt: Date.now(), updatedAt: Date.now(), messages: [] };
        set({ conversations: [conv, ...get().conversations], currentId: conv.id });
        convId = conv.id;
      }
      const user: Message = { id: uid(), role: "user", content, status: "done", createdAt: Date.now() };
      patchConversation(convId, (c) => ({
        ...c,
        title: c.messages.length === 0 ? titleFrom(content) : c.title,
        messages: [...c.messages, user],
      }));
      // Most recently used chats float to the top.
      const conversations = get().conversations;
      const idx = conversations.findIndex((c) => c.id === convId);
      if (idx > 0) set({ conversations: [conversations[idx], ...conversations.filter((_, i) => i !== idx)] });
      await generate(convId);
    },

    async regenerate() {
      const convId = get().currentId;
      if (!convId || get().generating) return;
      patchConversation(convId, (c) => {
        const msgs = [...c.messages];
        while (msgs.length && msgs[msgs.length - 1].role === "assistant") msgs.pop();
        return { ...c, messages: msgs };
      });
      await generate(convId);
    },

    async stop() {
      const id = get().generating;
      if (id) await api.chatCancel(id);
    },

    setMode(mode) {
      set({ mode });
    },
    setThinking(thinking) {
      set({ thinking });
    },
    toggleSidebar() {
      set({ sidebarOpen: !get().sidebarOpen });
    },
    openSettings(settingsTab) {
      set({ settingsTab });
      if (settingsTab === "models") void get().refreshModels();
    },
  };

  function handleDownload(e: DownloadEvent) {
    const prev = get().downloads[e.id];
    const base: DownloadState = prev ?? { phase: "downloading", bytes: 0, total: 0, bytesPerSec: 0 };
    let next: DownloadState;
    switch (e.kind) {
      case "resuming":
        next = { ...base, phase: "resuming", bytes: e.bytes, error: undefined };
        break;
      case "progress":
        next = { phase: "downloading", bytes: e.bytes, total: e.total, bytesPerSec: e.bytesPerSec };
        break;
      case "verifying":
        next = { ...base, phase: "verifying" };
        break;
      case "paused":
        next = { ...base, phase: "paused", bytes: e.bytes, bytesPerSec: 0 };
        break;
      case "failed":
        next = { ...base, phase: "failed", error: e.message, bytesPerSec: 0 };
        break;
      case "finished":
        next = { ...base, phase: "finished", bytes: base.total || base.bytes };
        break;
    }
    set({ downloads: { ...get().downloads, [e.id]: next } });
    if (e.kind === "finished" || e.kind === "failed" || e.kind === "paused") void get().refreshModels();
  }
});

export const currentConversation = (s: State) => s.conversations.find((c) => c.id === s.currentId) ?? null;
