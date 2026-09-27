import { create } from "zustand";

import { api, errorText, events, inTauri } from "../lib/api";
import { titleFrom } from "../lib/format";
import type {
  ChatEvent,
  ConversationMeta,
  DownloadEvent,
  EngineStatus,
  LoadedModel,
  Mode,
  ModelStatus,
  Settings,
  Source,
  Stats,
  SystemInfo,
  ThinkingPref,
  WireMessage,
} from "../lib/types";

/** One tool use shown in the answer's activity list. */
export interface Step {
  id: string;
  name: string;
  args: Record<string, unknown>;
  status: "running" | "ok" | "error";
  summary?: string;
  /** For "remember" suggestions: what the user decided. */
  decision?: "saved" | "dismissed";
}

export interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  reasoning?: string;
  thinking?: boolean;
  status: "streaming" | "done" | "error" | "cancelled";
  error?: string;
  stats?: Stats;
  steps?: Step[];
  sources?: Source[];
  mode?: Mode;
  model?: string;
  /** Answers to the same question from several models share a group id. */
  group?: string;
  /** A side-by-side answer that isn't sent back as history (the main model's is). */
  alt?: boolean;
  /** The user chose a model other than the main one for this answer. */
  picked?: boolean;
  createdAt: number;
}

export interface Conversation {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: Message[];
  pinned?: boolean;
  folder?: string | null;
  /** Private chats are never saved and don't use memory. */
  private?: boolean;
  /** Messages have been loaded from the database. */
  loaded?: boolean;
  /** Message count from the database (before messages are loaded). */
  messageCount?: number;
}

/** Has anything been said in this chat (loaded or not)? */
export const hasMessages = (c: Conversation) => c.messages.length > 0 || (c.messageCount ?? 0) > 0;

export interface DownloadState {
  phase: "resuming" | "downloading" | "verifying" | "paused" | "failed" | "finished";
  bytes: number;
  total: number;
  bytesPerSec: number;
  error?: string;
}

export type SettingsTab = "models" | "memory" | "appearance" | "engine" | "about";

interface State {
  ready: boolean;
  settings: Settings | null;
  system: SystemInfo | null;
  models: ModelStatus[];
  /** Key of the model + version BYTE recommends for this Mac. */
  recommended: string | null;
  engine: EngineStatus;
  downloads: Record<string, DownloadState>;
  conversations: Conversation[];
  currentId: string | null;
  generating: string | null;
  /** Every answer currently streaming (several when comparing models). */
  running: string[];
  /** Models in memory: the main one and any loaded alongside. */
  loaded: LoadedModel[];
  /** Who answers the next message: "main", a loaded model's key, or "compare". */
  answerWith: string;
  mode: Mode;
  thinking: ThinkingPref;
  sidebarOpen: boolean;
  settingsTab: SettingsTab | null;

  init(): Promise<void>;
  updateSettings(patch: Partial<Settings>): Promise<void>;
  refreshModels(): Promise<void>;
  refreshLoaded(): Promise<void>;
  setAnswerWith(v: string): void;
  newChat(isPrivate?: boolean): void;
  selectChat(id: string): Promise<void>;
  deleteChat(id: string): void;
  updateChat(id: string, patch: { title?: string; pinned?: boolean; folder?: string }): Promise<void>;
  /** Save or dismiss a "remember" suggestion shown under an answer. */
  resolveMemory(msgId: string, stepId: string, save: boolean): Promise<void>;
  /** Reload the chat list from the database (after an erase or import). */
  reloadChats(): Promise<void>;
  send(text: string): Promise<void>;
  regenerate(): Promise<void>;
  stop(): Promise<void>;
  toggleWeb(): void;
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
/** Browser-only fallback (dev without Tauri): keep chats in localStorage. */
function saveConversationsLocally(list: Conversation[]) {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(list.filter((c) => c.messages.length > 0 && !c.private)));
    } catch {
      /* storage full or unavailable: chats stay in memory */
    }
  }, 400);
}

/** What gets written to the database for a chat. */
function toStored(c: Conversation) {
  const { loaded: _l, messageCount: _n, private: _p, ...rest } = c;
  return rest;
}

const pendingSaves = new Map<string, ReturnType<typeof setTimeout>>();
/** Saves one chat to the encrypted database, debounced (longer while streaming). */
function scheduleSave(c: Conversation, delay: number) {
  if (c.private || c.messages.length === 0 || !c.loaded) return;
  clearTimeout(pendingSaves.get(c.id));
  pendingSaves.set(
    c.id,
    setTimeout(() => {
      pendingSaves.delete(c.id);
      api.chatSave(toStored(c)).catch((e) => console.error("couldn't save chat", e));
    }, delay),
  );
}

/** Moves chats kept by older builds in localStorage into the database, once. */
async function importLocalChats() {
  const old = loadConversations();
  if (old.length === 0) return;
  try {
    await api.chatsImport(old);
    localStorage.removeItem(STORAGE_KEY);
  } catch (e) {
    console.error("couldn't import old chats", e);
  }
}

const fromMeta = (m: ConversationMeta): Conversation => ({
  id: m.id,
  title: m.title,
  createdAt: m.createdAt,
  updatedAt: m.updatedAt,
  pinned: m.pinned,
  folder: m.folder,
  messageCount: m.messageCount,
  messages: [],
  loaded: false,
});

const uid = () => crypto.randomUUID();

/** Messages sent to the model: finished turns only, in order. Side-by-side
 * alternatives are left out so each question has one answer in the history. */
export function toWire(messages: Message[]): WireMessage[] {
  return messages
    .filter((m) => m.role === "user" || (!m.alt && m.status === "done" && m.content.trim().length > 0))
    .map((m) => ({ role: m.role, content: m.content }));
}

export const useStore = create<State>((set, get) => {
  const patchConversation = (id: string, fn: (c: Conversation) => Conversation) => {
    const conversations = get().conversations.map((c) => (c.id === id ? fn(c) : c));
    set({ conversations });
    if (!inTauri) {
      saveConversationsLocally(conversations);
      return;
    }
    const c = conversations.find((x) => x.id === id);
    if (c) scheduleSave(c, get().running.length ? 2000 : 300);
  };

  const patchMessage = (convId: string, msgId: string, fn: (m: Message) => Message) =>
    patchConversation(convId, (c) => ({
      ...c,
      updatedAt: Date.now(),
      messages: c.messages.map((m) => (m.id === msgId ? fn(m) : m)),
    }));

  /** Streams an assistant reply for the conversation's current history.
   * `model` picks a loaded model other than the main one. */
  const generate = async (convId: string, opts: { model?: string; group?: string; alt?: boolean } = {}) => {
    const conv = get().conversations.find((c) => c.id === convId);
    if (!conv) return;
    const { mode, thinking } = get();
    const history = toWire(conv.messages);
    const reply: Message = {
      id: uid(),
      role: "assistant",
      content: "",
      reasoning: "",
      status: "streaming",
      mode,
      model: opts.model,
      group: opts.group,
      alt: opts.alt,
      picked: !!opts.model && !opts.group,
      createdAt: Date.now(),
    };
    patchConversation(convId, (c) => ({ ...c, messages: [...c.messages, reply] }));
    set({ generating: get().generating ?? reply.id, running: [...get().running, reply.id] });

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
        case "toolCall":
          cancelAnimationFrame(frame);
          flush();
          patchMessage(convId, reply.id, (m) => ({
            ...m,
            steps: [...(m.steps ?? []), { id: e.id, name: e.name, args: e.args, status: "running" }],
          }));
          break;
        case "toolResult":
          patchMessage(convId, reply.id, (m) => ({
            ...m,
            steps: (m.steps ?? []).map((st) =>
              st.id === e.id ? { ...st, status: e.ok ? "ok" : "error", summary: e.summary } : st,
            ),
          }));
          break;
        case "sources":
          patchMessage(convId, reply.id, (m) => ({ ...m, sources: e.sources }));
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
      await api.chatSend({ requestId: reply.id, messages: history, mode, thinking, model: opts.model, private: conv.private }, onEvent);
    } catch (err) {
      cancelAnimationFrame(frame);
      flush();
      patchMessage(convId, reply.id, (m) => ({ ...m, status: "error", error: errorText(err) }));
    } finally {
      const running = get().running.filter((id) => id !== reply.id);
      set({ running, generating: running.length ? (get().generating === reply.id ? running[0] : get().generating) : null });
      const done = get().conversations.find((c) => c.id === convId);
      if (done && inTauri) scheduleSave(done, 100);
    }
  };

  /** Answers the last question with the model(s) chosen in the composer. */
  const answer = async (convId: string) => {
    const { answerWith, loaded } = get();
    const ready = loaded.filter((l) => l.status.state === "ready");
    if (answerWith === "compare" && ready.length > 1) {
      // The main model answers first; its answer is the one kept in history.
      const keys = [...ready].sort((a, b) => Number(b.primary) - Number(a.primary)).map((l) => l.key);
      const group = uid();
      await Promise.all(keys.map((model, i) => generate(convId, { model, group, alt: i > 0 })));
      return;
    }
    const pick = ready.find((l) => l.key === answerWith && !l.primary);
    await generate(convId, { model: pick?.key });
  };

  return {
    ready: false,
    settings: null,
    system: null,
    models: [],
    recommended: null,
    engine: { state: "stopped" },
    downloads: {},
    conversations: [],
    currentId: null,
    generating: null,
    running: [],
    loaded: [],
    answerWith: "main",
    mode: "auto",
    thinking: "auto",
    sidebarOpen: true,
    settingsTab: null,

    async init() {
      if (get().ready) return;
      if (!inTauri) {
        const conversations = loadConversations().map((c) => ({ ...c, loaded: true }));
        set({ ready: true, conversations, currentId: conversations[0]?.id ?? null });
        return;
      }
      await importLocalChats();
      const conversations = (await api.chatsList().catch(() => [] as ConversationMeta[])).map(fromMeta);
      await events.onEngineStatus((engine) => {
        set({ engine });
        void get().refreshLoaded();
        if (engine.state === "ready" || engine.state === "noModel") void get().refreshModels();
      });
      await events.onExtras(() => {
        void get().refreshLoaded();
        void get().refreshModels();
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
        currentId: null,
        mode: settings.defaultMode,
        thinking: settings.thinking,
      });
      void get().refreshLoaded();
      // Open the most recent chat.
      const first = conversations.find((c) => !c.pinned) ?? conversations[0];
      if (first) await get().selectChat(first.id);
    },

    async updateSettings(patch) {
      const settings = await api.settingsUpdate(patch);
      set({ settings });
    },

    async refreshLoaded() {
      if (!inTauri) return;
      const loaded = await api.modelsLoaded().catch(() => get().loaded);
      // Forget a choice whose model was unloaded.
      const { answerWith } = get();
      const still = answerWith === "main" || (answerWith === "compare" ? loaded.length > 1 : loaded.some((l) => l.key === answerWith));
      set({ loaded, answerWith: still ? answerWith : "main" });
    },

    setAnswerWith(answerWith) {
      set({ answerWith });
    },

    async refreshModels() {
      if (!inTauri) return;
      const [models, recommended] = await Promise.all([api.modelsList(), api.modelRecommend().catch(() => null)]);
      set({ models, recommended });
    },

    newChat(isPrivate = false) {
      // Reuse an empty chat of the same kind instead of stacking empty ones.
      const empty = get().conversations.find((c) => c.messages.length === 0 && !hasMessages(c) && !!c.private === isPrivate);
      if (empty) {
        set({ currentId: empty.id });
        return;
      }
      const conv: Conversation = {
        id: uid(),
        title: isPrivate ? "Private chat" : "New chat",
        createdAt: Date.now(),
        updatedAt: Date.now(),
        messages: [],
        private: isPrivate,
        loaded: true,
      };
      set({ conversations: [conv, ...get().conversations], currentId: conv.id });
    },

    async selectChat(id) {
      set({ currentId: id });
      const c = get().conversations.find((x) => x.id === id);
      if (!c || c.loaded || !inTauri) return;
      try {
        const full = (await api.chatLoad(id)) as Conversation | null;
        const messages = (full?.messages ?? []).map((m) => (m.status === "streaming" ? { ...m, status: "cancelled" as const } : m));
        set({ conversations: get().conversations.map((x) => (x.id === id ? { ...x, messages, loaded: true } : x)) });
      } catch (e) {
        console.error("couldn't load chat", e);
      }
    },

    deleteChat(id) {
      const target = get().conversations.find((c) => c.id === id);
      const conversations = get().conversations.filter((c) => c.id !== id);
      set({ conversations, currentId: get().currentId === id ? null : get().currentId });
      clearTimeout(pendingSaves.get(id));
      if (!inTauri) saveConversationsLocally(conversations);
      else if (target && !target.private) void api.chatDelete(id);
    },

    async updateChat(id, patch) {
      const c = get().conversations.find((x) => x.id === id);
      if (!c) return;
      set({
        conversations: get().conversations.map((x) =>
          x.id === id
            ? {
                ...x,
                title: patch.title?.trim() || x.title,
                pinned: patch.pinned ?? x.pinned,
                folder: patch.folder === undefined ? x.folder : patch.folder.trim() || null,
              }
            : x,
        ),
      });
      if (inTauri && !c.private) await api.chatUpdate(id, patch);
    },

    async resolveMemory(msgId, stepId, save) {
      const conv = currentConversation(get());
      const msg = conv?.messages.find((m) => m.id === msgId);
      const step = msg?.steps?.find((st) => st.id === stepId);
      if (!conv || !step) return;
      if (save) await api.memoryAdd(step.summary ?? "", "chat");
      patchMessage(conv.id, msgId, (m) => ({
        ...m,
        steps: (m.steps ?? []).map((st) => (st.id === stepId ? { ...st, decision: save ? "saved" : "dismissed" } : st)),
      }));
    },

    async reloadChats() {
      if (!inTauri) return;
      const conversations = (await api.chatsList()).map(fromMeta);
      set({ conversations, currentId: null });
    },

    async send(text) {
      const content = text.trim();
      if (!content || get().generating) return;
      let convId = get().currentId;
      if (!convId || !get().conversations.some((c) => c.id === convId)) {
        const conv: Conversation = { id: uid(), title: "New chat", createdAt: Date.now(), updatedAt: Date.now(), messages: [], loaded: true };
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
      await answer(convId);
    },

    async regenerate() {
      const convId = get().currentId;
      if (!convId || get().generating) return;
      patchConversation(convId, (c) => {
        const msgs = [...c.messages];
        while (msgs.length && msgs[msgs.length - 1].role === "assistant") msgs.pop();
        return { ...c, messages: msgs };
      });
      await answer(convId);
    },

    toggleWeb() {
      const s = get().settings;
      if (s) void get().updateSettings({ webSearch: !s.webSearch });
    },

    async stop() {
      await Promise.all(get().running.map((id) => api.chatCancel(id)));
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
