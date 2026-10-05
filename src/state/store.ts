import { create } from "zustand";

import { nextWeb, webState } from "../lib/web";
import { chime } from "../lib/sounds";
import { api, errorText, events, inTauri, type ChatPatch, type CloudTurn } from "../lib/api";
import { idOf, isImage, listOf, str, titleOf } from "../lib/cloudDocs";
import { branchAt, switchVersion, versionsAt } from "../lib/branches";
import { titleFrom } from "../lib/format";
import { endBrowsing } from "../lib/agent";
import { isWindows } from "../lib/keys";
import type {
  NoteInput,
  ChatEvent,
  ConversationMeta,
  DownloadEvent,
  EngineStatus,
  LoadedModel,
  Project,
  TuneProgress,
  Mode,
  ModelStatus,
  Settings,
  Source,
  Stats,
  SystemInfo,
  ThinkingPref,
  WireMessage,
  LocalFile,
  KbProgress,
  KbStatus,
  CloudStatus,
  Workspace,
  ChatTask,
  Decision,
  PlacesFound,
  TripPlan,
  Recipe,
  RecipeIdeas,
  MealPlan,
  VideoCard,
  ApprovalCard,
  SavedFile,
  MacDone,
  RunCard,
  Storage,
  Health,
  Reviews,
  Prices,
  GameHints,
  SelfCheck,
  Flashcards,
  Quiz,
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

/** What the reader panel shows: a file from disk (`path`) or text already read (`text`). */
export interface ReaderDoc {
  title: string;
  path?: string;
  text?: string;
  page?: number | null;
  /** Passage to highlight and scroll to. */
  highlight?: string;
}

/** A photo or file sent with a message (stored on the BYTE cloud). */
export interface Attachment {
  id: string;
  name: string;
  image: boolean;
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
  /** The user chose this side-by-side answer to continue the chat. */
  kept?: boolean;
  /** Other versions of the thread from this message on (edit & regenerate). */
  alts?: Message[][];
  /** This version's place among all versions (0-based). */
  version?: number;
  /** BYTE was closed while this answer was being written. */
  interrupted?: boolean;
  /** Written on the BYTE cloud; `remoteId` is its message id there. */
  cloud?: boolean;
  remoteId?: string;
  /** Cloud mode id used for this answer. */
  cloudMode?: string;
  /** Live progress from the cloud while it works ("searching: …"). */
  phase?: string;
  /** Something to point out about this answer (e.g. written locally because the cloud was down). */
  notice?: string;
  /** Compare & decide score table shown above the answer. */
  decision?: Decision;
  /** Places found nearby, shown as cards. */
  places?: PlacesFound;
  /** A trip plan card. */
  trip?: TripPlan;
  /** Kitchen cards. */
  recipe?: Recipe;
  recipeIdeas?: RecipeIdeas;
  mealPlan?: MealPlan;
  /** A YouTube video summary card. */
  video?: VideoCard;
  /** Web agent: approval cards (submit, download…), files it saved, and whether its browser is open. */
  approvals?: ApprovalCard[];
  saved?: SavedFile[];
  /** What BYTE did in Mac apps (Mac control), with Undo tokens. */
  mac?: MacDone[];
  /** An automation started from this answer (its card follows the run). */
  automationRun?: RunCard;
  browsing?: boolean;
  /** Reviews, prices and game-hint cards; the self-check note under the answer. */
  reviews?: Reviews;
  prices?: Prices;
  hints?: GameHints;
  selfCheck?: SelfCheck;
  /** Study cards made in chat. */
  flashcards?: Flashcards;
  quiz?: Quiz;
  /** Mac upkeep cards: storage and health. */
  storage?: Storage;
  health?: Health;
  /** A job this (user) message asked for with a button, e.g. Fact-check. */
  task?: ChatTask;
  /** Thumbs up/down given on the cloud. */
  feedback?: "up" | "down";
  /** Photos/files sent with this (user) message. */
  attachments?: Attachment[];
  /** Files read on this Mac and sent with this (user) message (local chats). */
  files?: LocalFile[];
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
  summary?: string | null;
  tags?: string[];
  projectId?: string | null;
  /** The custom assistant this chat was started with. */
  assistantId?: string | null;
  /** Conversation id on the BYTE cloud once this chat has used it. */
  cloudId?: string | null;
  /** Newest message the cloud conversation continues from (this session). */
  cloudHead?: string | null;
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

export type SettingsTab = "models" | "memory" | "knowledge" | "connectors" | "appearance" | "engine" | "cloud" | "privacy" | "about";

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
  /** The answer BYTE is reading aloud (speech.rs), if any. */
  speakingId: string | null;
  /** Hands-free conversation: answers are read aloud and the mic opens again after each. */
  talk: boolean;
  /** BYTE's own voices (tts.rs) are downloaded: answers start speaking while they're written. */
  voicesReady: boolean;
  /** The last answer that finished (hands-free and read-aloud react to it). */
  lastAnswer: { id: string; ok: boolean; at: number } | null;
  /** "Tune for this Mac" progress while it runs (chat waits meanwhile). */
  tune: TuneProgress | null;
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
  newChat(isPrivate?: boolean, projectId?: string | null, assistant?: { id: string; mode: string } | null): void;
  selectChat(id: string): Promise<void>;
  deleteChat(id: string): void;
  updateChat(id: string, patch: ChatPatch): Promise<void>;
  /** Replace a user message and answer again; the old version is kept. */
  editMessage(msgId: string, text: string): Promise<void>;
  /** Show another version of the thread at this message. */
  showVersion(msgId: string, index: number): void;
  projects: Project[];
  refreshProjects(): Promise<void>;
  saveProject(p: Project): Promise<Project>;
  deleteProject(id: string): Promise<void>;
  /** Save or dismiss a "remember" suggestion shown under an answer. */
  resolveMemory(msgId: string, stepId: string, save: boolean): Promise<void>;
  /** Reload the chat list from the database (after an erase or import). */
  reloadChats(): Promise<void>;
  speak(id: string, text: string): Promise<void>;
  stopSpeaking(): void;
  setTalk(on: boolean): void;
  /** Adds chats saved elsewhere (Quick Ask, schedules) without leaving the open one. */
  addNewChats(): Promise<void>;
  /** Re-reads one chat from disk (another window changed it) and opens it. */
  openFresh(id: string): Promise<void>;
  /** `spoken`: asked out loud ("Hey BYTE"): the answer is spoken whatever the Read aloud setting. */
  send(text: string, opts?: { task?: ChatTask; spoken?: boolean }): Promise<void>;
  /** A spoken question is being answered (its answer is read aloud). */
  voiceTurn: boolean;
  regenerate(): Promise<void>;
  /** Cloud account status (connected, modes). */
  cloud: CloudStatus | null;
  /** Conversations on the BYTE cloud (Cloud workspace sidebar); null until loaded. */
  cloudChats: CloudChat[] | null;
  /** Something to tell the user about the cloud workspace (e.g. deleting isn't supported). */
  cloudNotice: string | null;
  refreshCloudChats(): Promise<void>;
  /** Deletes a conversation on the BYTE cloud (not the copy on this Mac). */
  deleteCloudChat(cloudId: string): Promise<void>;
  openCloudChat(cloudId: string): Promise<void>;
  setWorkspace(ws: Workspace): Promise<void>;
  /** Makes this side-by-side answer the one that continues the conversation. */
  keepAnswer(msgId: string): void;
  /** Photos/files waiting to go with the next message (cloud chats). */
  pending: Attachment[];
  /** Uploads in progress, and the last upload error. */
  attaching: number;
  attachError: string | null;
  attachFiles(paths: string[]): Promise<void>;
  /** Files read on this Mac, waiting to go with the next message (local chats). */
  pendingFiles: LocalFile[];
  /** Reads files for a local chat (photos only when the loaded model can see). */
  attachLocal(paths: string[]): Promise<void>;
  removePendingFile(index: number): void;
  /** Saved prompts from the cloud, used as "/" commands in the chat box. */
  savedPrompts: { id: string; title: string; text: string }[] | null;
  loadSavedPrompts(force?: boolean): Promise<void>;
  attachExisting(a: Attachment): void;
  removePending(id: string): void;
  refreshCloud(): Promise<void>;
  /** The reader side panel: a file's text with the cited passage highlighted. */
  reader: ReaderDoc | null;
  /** The Study panel: closed (null), or open on a deck (or the deck list). */
  study: { deck: number | null } | null;
  openStudy(deck?: number | null): void;
  closeStudy(): void;
  /** The writing studio, with the text it opened with (and the app it came from: the ⌥⌘B hotkey). */
  /** Text to put in the message box (an example prompt); the Composer takes it and clears it. */
  prefill: string | null;
  /** The brainstorm board: closed (null), or open (on a topic to start with ideas). */
  board: { topic?: string; seq: number } | null;
  openBoard(topic?: string): void;
  closeBoard(): void;
  /** The help center: closed (null) or open on an article. */
  help: string | null;
  openHelp(article?: string): void;
  closeHelp(): void;
  /** The Notes panel: closed (null), or open on a note id, or on a new note's draft. */
  notes: { id?: string; draft?: NoteInput; seq: number } | null;
  openNotes(opts?: { id?: string; draft?: NoteInput }): void;
  closeNotes(): void;
  /** A mind map being shown: the text it's made from, its title, and the chat it came from. */
  mindmap: { text: string; title: string; chat?: string } | null;
  openMindmap(text: string, title: string, chat?: string): void;
  closeMindmap(): void;
  writing: { text: string; app?: string; notice?: string; seq: number } | null;
  openWriting(text?: string, app?: string, notice?: string): void;
  closeWriting(): void;
  openReader(doc: ReaderDoc): void;
  closeReader(): void;
  /** Knowledge base folders and search-model state; `kbProgress` while indexing. */
  kb: KbStatus | null;
  kbProgress: KbProgress | null;
  refreshKb(): Promise<void>;
  toggleFiles(): void;
  /** Deepen / justify (new answer), answer-now / stop, or thumbs up/down on a cloud answer. */
  cloudAct(msgId: string, action: "deepen" | "justify" | "answer-now" | "feedback", value?: "up" | "down"): Promise<void>;
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
  summary: m.summary,
  tags: m.tags,
  projectId: m.projectId,
  assistantId: m.assistantId ?? null,
  cloudId: m.cloudId ?? null,
  messages: [],
  loaded: false,
});

const uid = () => crypto.randomUUID();

/**
 * Which workspace a chat belongs to, from its id: chats made in the Cloud
 * workspace (or imported from the cloud) start with "cloud-", chats in Both
 * with "both-"; everything else is This Mac.
 */
export const spaceOf = (id: string): Workspace => (id.startsWith("cloud-") ? "cloud" : id.startsWith("both-") ? "both" : "local");

/** The workspace in use (Cloud and Both need a connected account). */
export const workspaceOf = (s: Settings | null): Workspace => {
  if (!s?.cloudConnected) return "local";
  if (s.workspace === "cloud" || s.workspace === "both") return s.workspace;
  return s.useCloud ? "cloud" : "local";
};

/** The cloud's conversation list, whatever shape it comes in. */
export function cloudChatsFrom(v: unknown): CloudChat[] {
  return listOf(v)
    .map((r) => {
      const id = idOf(r);
      if (!id) return null;
      const when = r.updated_at ?? r.updatedAt ?? r.created_at ?? r.createdAt;
      const t = typeof when === "number" ? (when < 1e12 ? when * 1000 : when) : typeof when === "string" ? Date.parse(when) : NaN;
      return { id, title: titleOf(r), updatedAt: Number.isFinite(t) ? t : 0 };
    })
    .filter((c): c is CloudChat => c !== null)
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

/** New chat id for a workspace. */
const newId = (ws: Workspace) => (ws === "local" ? crypto.randomUUID() : `${ws}-${crypto.randomUUID()}`);

/** A conversation listed on the BYTE cloud. */
export interface CloudChat {
  id: string;
  title: string;
  updatedAt: number;
}

/** Cloud id of the newest message that has one. */
const lastRemoteId = (messages: Message[]) => [...messages].reverse().find((m) => m.remoteId)?.remoteId ?? null;

/**
 * Where the new question goes on the cloud. Normally it continues the cloud
 * conversation; after an edit or a regenerate the chat goes on from an
 * earlier message, so the cloud conversation is forked there first (or a new
 * one is started when the very first message changed).
 */
export function cloudTurn(conv: Pick<Conversation, "messages" | "cloudId" | "cloudHead">, mode: string): CloudTurn {
  const msgs = conv.messages;
  const last = msgs[msgs.length - 1];
  const before = last?.role === "user" ? msgs.slice(0, -1) : msgs;
  const prev = lastRemoteId(before);
  if (!conv.cloudId) return { conversationId: null, mode, lastRemoteId: null, branchFrom: null };
  const head = conv.cloudHead ?? lastRemoteId(msgs);
  const forked = !!last?.remoteId || (head !== null && head !== prev);
  if (!forked) return { conversationId: conv.cloudId, mode, lastRemoteId: prev, branchFrom: null };
  if (!prev) return { conversationId: null, mode, lastRemoteId: null, branchFrom: null };
  return { conversationId: conv.cloudId, mode, lastRemoteId: prev, branchFrom: prev };
}

/** Messages sent to the model: finished turns only, in order. Side-by-side
 * alternatives are left out so each question has one answer in the history. */
export function toWire(messages: Message[]): WireMessage[] {
  return messages
    .filter((m) => m.role === "user" || (!m.alt && m.status === "done" && m.content.trim().length > 0))
    .map((m) => ({ role: m.role, content: m.content, ...(m.role === "user" && m.files?.length ? { files: m.files } : {}) }));
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
  const generate = async (
    convId: string,
    opts: { model?: string; group?: string; alt?: boolean; cloud?: boolean; branch?: { alts: Message[][]; version: number } } = {},
  ) => {
    const conv = get().conversations.find((c) => c.id === convId);
    if (!conv) return;
    const { mode, thinking, settings } = get();
    const history = toWire(conv.messages);
    // Cloud chats answer on the BYTE cloud (never private chats or a picked local model); Both asks for it explicitly.
    const connected = !!settings?.cloudConnected && !conv.private;
    const useCloud = connected && (opts.cloud ?? (spaceOf(conv.id) === "cloud" && !opts.model && !opts.group));
    const cloudMode = settings?.cloudMode ?? get().cloud?.account?.modes[0]?.id ?? "auto";
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
      alts: opts.branch?.alts,
      version: opts.branch?.version,
      cloud: useCloud || undefined,
      cloudMode: useCloud ? cloudMode : undefined,
      createdAt: Date.now(),
    };
    patchConversation(convId, (c) => ({ ...c, messages: [...c.messages, reply] }));
    let cloud: CloudTurn | undefined;
    if (useCloud) {
      cloud = cloudTurn(conv, cloudMode);
      const asked = [...conv.messages].reverse().find((m) => m.role === "user");
      if (asked?.attachments?.length) cloud.attachmentIds = asked.attachments.map((a) => a.id);
      if (opts.cloud) cloud.noFallback = true;
    }
    // A button's job (Fact-check) rides on the user message, so Regenerate keeps it.
    const task = useCloud ? undefined : [...conv.messages].reverse().find((m) => m.role === "user")?.task;
    await streamReply(convId, reply, (onEvent) =>
      api.chatSend(
        // Regenerating (a new version of an answer) never reuses an earlier answer.
        { requestId: reply.id, messages: history, mode, thinking, model: opts.model, private: conv.private, projectId: conv.projectId, assistantId: conv.assistantId ?? null, cloud, fresh: !!opts.branch, task, spoken: spokenTurn() },
        onEvent,
      ),
    );
  };

  /** Runs one streamed answer into `reply` (a message already in the chat). */
  /** This answer will be heard: Read aloud is on, Talk mode, or a question asked out loud. */
  const spokenTurn = () => canSpeak() && !!(get().settings?.readAloud || get().talk || get().voiceTurn);

  const streamReply = async (convId: string, reply: Message, call: (onEvent: (e: ChatEvent) => void) => Promise<void>) => {
    set({ generating: get().generating ?? reply.id, running: [...get().running, reply.id] });

    // Batch token deltas into one render per animation frame.
    let pendingContent = "";
    let pendingReasoning = "";
    let frame = 0;
    let lastFeed = 0;
    const isPrivate = !!get().conversations.find((x) => x.id === convId)?.private;
    const streamVoice = () => spokenTurn();
    const flush = () => {
      frame = 0;
      if (!pendingContent && !pendingReasoning) return;
      const c = pendingContent;
      const r = pendingReasoning;
      pendingContent = "";
      pendingReasoning = "";
      patchMessage(convId, reply.id, (m) => ({ ...m, content: m.content + c, reasoning: (m.reasoning ?? "") + r }));
      // BYTE's voices start reading while the answer is still being written (finished sentences only).
      if (c && streamVoice()) {
        const text = get().conversations.find((x) => x.id === convId)?.messages.find((m) => m.id === reply.id)?.content ?? "";
        if (/[.!?]\s/.test(c) || Date.now() - lastFeed > 1500) {
          lastFeed = Date.now();
          set({ speakingId: reply.id });
          void api.speechFeed(reply.id, text, false, isPrivate).catch(() => undefined);
        }
      }
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(flush);
    };

    const onEvent = (e: ChatEvent) => {
      switch (e.kind) {
        case "started":
          patchMessage(convId, reply.id, (m) => ({ ...m, thinking: e.thinking, model: m.cloud ? undefined : e.model }));
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
        case "phase":
          patchMessage(convId, reply.id, (m) => ({ ...m, phase: e.text }));
          break;
        case "remote":
          patchConversation(convId, (c) => {
            const head = e.messageId ?? e.userMessageId ?? c.cloudHead ?? null;
            // The user's message gets its cloud id too, so later turns know where to continue.
            let userIdx = -1;
            c.messages.forEach((m, i) => {
              if (m.role === "user" && i < c.messages.findIndex((x) => x.id === reply.id)) userIdx = i;
            });
            return {
              ...c,
              cloudId: e.conversationId,
              cloudHead: head,
              messages: c.messages.map((m, i) =>
                m.id === reply.id && e.messageId
                  ? { ...m, remoteId: e.messageId }
                  : i === userIdx && e.userMessageId
                    ? { ...m, remoteId: e.userMessageId }
                    : m,
              ),
            };
          });
          break;
        case "notice":
          patchMessage(convId, reply.id, (m) => ({ ...m, notice: e.text, cloud: undefined, cloudMode: undefined }));
          break;
        case "decision": {
          const { kind: _kind, ...decision } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, decision }));
          break;
        }
        case "places": {
          const { kind: _kind, ...places } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, places }));
          break;
        }
        case "trip": {
          const { kind: _kind, ...trip } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, trip }));
          break;
        }
        case "recipe": {
          const { kind: _kind, ...recipe } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, recipe }));
          break;
        }
        case "recipeIdeas": {
          const { kind: _kind, ...recipeIdeas } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, recipeIdeas }));
          break;
        }
        case "mealPlan": {
          const { kind: _kind, ...mealPlan } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, mealPlan }));
          break;
        }
        case "video": {
          const { kind: _kind, ...video } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, video }));
          break;
        }
        case "approval": {
          const { kind: _kind, ...ask } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, approvals: [...(m.approvals ?? []), { ...ask, status: "waiting" }] }));
          break;
        }
        case "approvalDone":
          patchMessage(convId, reply.id, (m) => ({
            ...m,
            approvals: (m.approvals ?? []).map((a) => (a.id === e.id ? { ...a, status: e.ok ? "approved" : a.status === "waiting" ? "declined" : a.status } : a)),
          }));
          break;
        case "saved": {
          const { kind: _kind, ...file } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, saved: [...(m.saved ?? []), file] }));
          break;
        }
        case "automationRun": {
          const { kind: _kind, ...card } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, automationRun: card }));
          break;
        }
        case "macDone": {
          const { kind: _kind, ...done } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, mac: [...(m.mac ?? []), done] }));
          break;
        }
        case "browsing":
          patchMessage(convId, reply.id, (m) => ({ ...m, browsing: e.active }));
          break;
        case "reviews": {
          const { kind: _kind, ...reviews } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, reviews }));
          break;
        }
        case "prices": {
          const { kind: _kind, ...prices } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, prices }));
          break;
        }
        case "hints": {
          const { kind: _kind, ...hints } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, hints }));
          break;
        }
        case "flashcards": {
          const { kind: _kind, ...flashcards } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, flashcards }));
          break;
        }
        case "quiz": {
          const { kind: _kind, ...quiz } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, quiz }));
          break;
        }
        case "storage": {
          const { kind: _kind, ...storage } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, storage }));
          break;
        }
        case "health": {
          const { kind: _kind, ...health } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, health }));
          break;
        }
        case "selfCheck": {
          const { kind: _kind, ...selfCheck } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, selfCheck }));
          break;
        }
        case "stats": {
          const { kind: _kind, ...stats } = e;
          patchMessage(convId, reply.id, (m) => ({ ...m, stats }));
          break;
        }
        case "done":
          cancelAnimationFrame(frame);
          flush();
          if (get().settings?.sounds && e.finishReason !== "cancelled") chime("done");
          patchMessage(convId, reply.id, (m) => ({
            ...endBrowsing(m),
            phase: undefined,
            status: e.finishReason === "cancelled" ? "cancelled" : "done",
          }));
          break;
      }
    };

    try {
      await call(onEvent);
    } catch (err) {
      cancelAnimationFrame(frame);
      flush();
      patchMessage(convId, reply.id, (m) => ({ ...endBrowsing(m), phase: undefined, status: "error", error: errorText(err) }));
    } finally {
      const running = get().running.filter((id) => id !== reply.id);
      set({ running, generating: running.length ? (get().generating === reply.id ? running[0] : get().generating) : null });
      const done = get().conversations.find((c) => c.id === convId);
      const answer = done?.messages.find((m) => m.id === reply.id);
      const ok = answer?.status === "done" && !!answer.content.trim();
      set({ lastAnswer: { id: reply.id, ok, at: Date.now() } });
      // Read it aloud (setting, or hands-free conversation). Only the main answer of a chat, on a Mac.
      if (ok && answer && !answer.alt && spokenTurn()) {
        // Rust picks the voice: BYTE's own, the cloud's, or the Mac's.
        set({ speakingId: reply.id });
        void api.speechFeed(reply.id, answer.content, true, isPrivate).catch(() => set({ speakingId: null }));
      } else if (get().speakingId === reply.id) get().stopSpeaking();
      set({ voiceTurn: false });
      if (done && inTauri) {
        scheduleSave(done, 100);
        maybeAutotitle(done);
        rememberAnswer(done, reply.id);
      }
    }
  };

  /** A chat's first local answer is kept for instant reuse (answer_cache.rs decides
   * whether the question qualifies: timeless, standalone, no files). */
  const rememberAnswer = (c: Conversation, replyId: string) => {
    if (c.private || !get().settings?.answerCache) return;
    const [q, a] = c.messages;
    const ok = c.messages.length === 2 && q.role === "user" && !q.files?.length && !q.attachments?.length && a.id === replyId;
    if (!ok || a.status !== "done" || a.cloud || a.alt || a.group || a.model || a.notice || !a.content.trim()) return;
    void api.answerCachePut(q.content, a.mode ?? "auto", a.content, a.sources ?? []).catch((e) => console.warn("answer not cached", e));
  };

  /** After the first answer, BYTE titles, summarizes and tags the chat (once). */
  const titled = new Set<string>();
  const maybeAutotitle = (c: Conversation) => {
    if (c.private || c.summary || titled.has(c.id) || get().running.length) return;
    if (!c.messages.some((m) => m.role === "assistant" && m.status === "done" && m.content)) return;
    titled.add(c.id);
    setTimeout(() => {
      api
        .chatAutotitle(c.id)
        .then((s) => {
          if (!s) return;
          set({
            conversations: get().conversations.map((x) => (x.id === c.id ? { ...x, title: s.title, summary: s.summary, tags: s.tags } : x)),
          });
        })
        .catch((e) => console.warn("couldn't title chat", e));
    }, 1500);
  };

  /** Answers the last question with the model(s) chosen in the composer. */
  const answer = async (convId: string, branch?: { alts: Message[][]; version: number }) => {
    const { answerWith, loaded } = get();
    const conv = get().conversations.find((c) => c.id === convId);
    if (conv && spaceOf(conv.id) === "both" && !conv.private && get().settings?.cloudConnected) {
      // Both: this Mac answers right away, the cloud answers beside it. The cloud's
      // answer continues the chat when it finishes, unless the user picks otherwise.
      const group = uid();
      const before = new Set(conv.messages.map((m) => m.id));
      await Promise.all([generate(convId, { group }), generate(convId, { group, alt: true, cloud: true })]);
      const after = get().conversations.find((c) => c.id === convId);
      const fresh = after?.messages.filter((m) => !before.has(m.id) && m.group === group) ?? [];
      const cloudMsg = fresh.find((m) => m.cloud);
      if (cloudMsg?.status === "done" && cloudMsg.content.trim() && !fresh.some((m) => m.kept)) get().keepAnswer(cloudMsg.id);
      return;
    }
    const ready = loaded.filter((l) => l.status.state === "ready");
    if (answerWith === "compare" && ready.length > 1) {
      // The main model answers first; its answer is the one kept in history.
      const keys = [...ready].sort((a, b) => Number(b.primary) - Number(a.primary)).map((l) => l.key);
      const group = uid();
      await Promise.all(keys.map((model, i) => generate(convId, { model, group, alt: i > 0 })));
      return;
    }
    const pick = ready.find((l) => l.key === answerWith && !l.primary);
    await generate(convId, { model: pick?.key, branch });
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
    speakingId: null,
    talk: false,
    voiceTurn: false,
    lastAnswer: null,
    voicesReady: false,
    loaded: [],
    projects: [],
    tune: null,
    answerWith: "main",
    cloud: null,
    cloudChats: null,
    cloudNotice: null,
    pending: [],
    savedPrompts: null,
    attaching: 0,
    attachError: null,
    pendingFiles: [],
    kb: null,
    kbProgress: null,
    reader: null,
    study: null,
    openStudy: (deck) => set({ study: { deck: deck ?? null } }),
    closeStudy: () => set({ study: null }),
    prefill: null,
    board: null,
    openBoard: (topic) => set((s) => ({ board: { topic, seq: (s.board?.seq ?? 0) + 1 } })),
    closeBoard: () => set({ board: null }),
    help: null,
    openHelp: (article) => set({ help: article ?? "getting-started" }),
    closeHelp: () => set({ help: null }),
    notes: null,
    openNotes: (opts) => set((s) => ({ notes: { ...opts, seq: (s.notes?.seq ?? 0) + 1 } })),
    closeNotes: () => set({ notes: null }),
    mindmap: null,
    openMindmap: (text, title, chat) => set({ mindmap: { text, title, chat } }),
    closeMindmap: () => set({ mindmap: null }),
    writing: null,
    openWriting: (text, app, notice) => set((s) => ({ writing: { text: text ?? "", app, notice, seq: (s.writing?.seq ?? 0) + 1 } })),
    closeWriting: () => set({ writing: null }),
    openReader: (doc) => set({ reader: doc }),
    closeReader: () => set({ reader: null }),
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
      void get().refreshProjects();
      void get().refreshCloud();
      await events.onEngineStatus((engine) => {
        set({ engine });
        void get().refreshLoaded();
        if (engine.state === "ready" || engine.state === "noModel") void get().refreshModels();
      });
      await events.onTune((tune) => {
        set({ tune: tune.done ? null : tune });
        if (tune.done) void api.settingsGet().then((settings) => set({ settings }));
      });
      await events.onExtras(() => {
        void get().refreshLoaded();
        void get().refreshModels();
      });
      await events.onDownload((e) => handleDownload(e));
      await api.onSpeechDone(() => set({ speakingId: null }));
      void api.voicesStatus().then((t) => set({ voicesReady: t.ready.length > 0 }), () => undefined);
      void get().refreshKb();
      await events.onKbProgress((p) => {
        set({ kbProgress: p.phase === "done" ? null : p });
        if (p.phase === "done" || p.done % 25 === 0) void get().refreshKb();
      });
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
      const ws = workspaceOf(settings);
      const mine = conversations.filter((c) => spaceOf(c.id) === ws);
      const first = mine.find((c) => !c.pinned) ?? mine[0];
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

    newChat(isPrivate = false, projectId = null, assistant = null) {
      const assistantId = assistant?.id ?? null;
      // An assistant's default mode applies to its chats.
      if (assistant?.mode && ["fast", "auto", "deep", "extended"].includes(assistant.mode)) get().setMode(assistant.mode as Mode);
      // Reuse an empty chat of the same kind instead of stacking empty ones.
      const ws = workspaceOf(get().settings);
      const empty = get().conversations.find(
        (c) =>
          c.messages.length === 0 &&
          !hasMessages(c) &&
          !!c.private === isPrivate &&
          (c.projectId ?? null) === projectId &&
          (c.assistantId ?? null) === assistantId &&
          (isPrivate || spaceOf(c.id) === ws),
      );
      if (empty) {
        set({ currentId: empty.id, pending: [], pendingFiles: [] });
        return;
      }
      const conv: Conversation = {
        id: isPrivate ? uid() : newId(workspaceOf(get().settings)),
        title: isPrivate ? "Private chat" : "New chat",
        createdAt: Date.now(),
        updatedAt: Date.now(),
        messages: [],
        private: isPrivate,
        projectId,
        assistantId,
        loaded: true,
      };
      set({ conversations: [conv, ...get().conversations], currentId: conv.id, pending: [], pendingFiles: [] });
    },

    async selectChat(id) {
      set({ currentId: id, pending: get().currentId === id ? get().pending : [] });
      const c = get().conversations.find((x) => x.id === id);
      if (!c || c.loaded || !inTauri) return;
      try {
        const full = (await api.chatLoad(id)) as Conversation | null;
        const messages = (full?.messages ?? []).map((m) =>
          m.status === "streaming" ? { ...m, status: "cancelled" as const, interrupted: true } : m,
        );
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
      // A chat in the Cloud workspace is deleted on the cloud too.
      if (inTauri && target?.cloudId && spaceOf(id) === "cloud") void get().deleteCloudChat(target.cloudId);
    },

    async deleteCloudChat(cloudId) {
      set({ cloudChats: get().cloudChats?.filter((c) => c.id !== cloudId) ?? null });
      try {
        await api.cloudDelete(`/api/conversations/${encodeURIComponent(cloudId)}`);
        set({ cloudNotice: null });
      } catch (e) {
        const text = errorText(e);
        set({
          cloudNotice: /\b(404|405)\b|not found|not allowed/i.test(text)
            ? "Your cloud doesn't support deleting chats yet, so it's only hidden here. It stays on the cloud."
            : `Couldn't delete it on the cloud: ${text}`,
        });
      }
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
                projectId: patch.projectId === undefined ? x.projectId : patch.projectId || null,
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

    async speak(id, text) {
      try {
        set({ speakingId: id });
        await api.speechSay(text);
      } catch (e) {
        set({ speakingId: null });
        console.warn("couldn't read aloud", e);
      }
    },

    stopSpeaking() {
      set({ speakingId: null });
      if (inTauri) void api.speechStop();
    },

    setTalk(talk) {
      set({ talk });
      if (!talk) get().stopSpeaking();
    },

    async addNewChats() {
      if (!inTauri) return;
      const list = (await api.chatsList().catch(() => [] as ConversationMeta[])).map(fromMeta);
      const have = new Set(get().conversations.map((c) => c.id));
      const fresh = list.filter((c) => !have.has(c.id));
      if (fresh.length) set({ conversations: [...fresh, ...get().conversations] });
    },

    async openFresh(id) {
      await get().addNewChats();
      // Not loaded: selectChat reads it from disk again.
      set({ conversations: get().conversations.map((c) => (c.id === id && !get().running.length ? { ...c, loaded: false } : c)) });
      await get().selectChat(id);
    },

    async send(text, opts) {
      const content = text.trim();
      if (!content || get().generating) return;
      set({ voiceTurn: !!opts?.spoken });
      let convId = get().currentId;
      if (!convId || !get().conversations.some((c) => c.id === convId)) {
        const conv: Conversation = { id: newId(workspaceOf(get().settings)), title: "New chat", createdAt: Date.now(), updatedAt: Date.now(), messages: [], loaded: true };
        set({ conversations: [conv, ...get().conversations], currentId: conv.id });
        convId = conv.id;
      }
      const attachments = get().pending;
      const files = get().pendingFiles;
      const user: Message = {
        id: uid(),
        role: "user",
        content,
        status: "done",
        createdAt: Date.now(),
        ...(attachments.length ? { attachments } : {}),
        ...(files.length ? { files } : {}),
        ...(opts?.task ? { task: opts.task } : {}),
      };
      set({ pending: [], pendingFiles: [], attachError: null });
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
      const conv = currentConversation(get());
      if (!convId || !conv || get().generating) return;
      // The earlier answer(s) stay available as other versions.
      let i = conv.messages.length;
      while (i > 0 && conv.messages[i - 1].role === "assistant") i--;
      const grouped = conv.messages.slice(i).some((m) => m.group);
      const branch = i < conv.messages.length && !grouped ? versionsAt(conv.messages, i) : undefined;
      patchConversation(convId, (c) => ({ ...c, messages: c.messages.slice(0, i) }));
      await answer(convId, branch ? { alts: branch, version: branch.length } : undefined);
    },

    async editMessage(msgId, text) {
      const conv = currentConversation(get());
      const content = text.trim();
      if (!conv || !content || get().generating) return;
      const i = conv.messages.findIndex((m) => m.id === msgId);
      if (i < 0 || conv.messages[i].role !== "user") return;
      const { attachments: kept, files } = conv.messages[i];
      const edited: Message = {
        id: uid(),
        role: "user",
        content,
        status: "done",
        createdAt: Date.now(),
        ...(kept ? { attachments: kept } : {}),
        ...(files ? { files } : {}),
      };
      patchConversation(conv.id, (c) => ({ ...c, updatedAt: Date.now(), messages: branchAt(c.messages, i, edited) }));
      await answer(conv.id);
    },

    showVersion(msgId, index) {
      const conv = currentConversation(get());
      if (!conv || get().generating) return;
      const i = conv.messages.findIndex((m) => m.id === msgId);
      if (i < 0) return;
      patchConversation(conv.id, (c) => ({ ...c, messages: switchVersion(c.messages, i, index) }));
    },

    async refreshProjects() {
      if (!inTauri) return;
      set({ projects: await api.projectsList().catch(() => get().projects) });
    },

    async saveProject(p) {
      const saved = inTauri ? await api.projectSave(p) : { ...p, id: p.id || uid(), createdAt: p.createdAt || Date.now() };
      const others = get().projects.filter((x) => x.id !== saved.id);
      set({ projects: [...others, saved].sort((a, b) => a.name.localeCompare(b.name)) });
      return saved;
    },

    async deleteProject(id) {
      if (inTauri) await api.projectDelete(id);
      set({
        projects: get().projects.filter((p) => p.id !== id),
        conversations: get().conversations.map((c) => (c.projectId === id ? { ...c, projectId: null } : c)),
      });
    },

    async refreshCloud() {
      if (!inTauri) return;
      set({ cloud: await api.cloudStatus().catch(() => get().cloud) });
    },

    async refreshCloudChats() {
      if (!inTauri || !get().settings?.cloudConnected) return;
      try {
        set({ cloudChats: cloudChatsFrom(await api.cloudConversations()), cloudNotice: null });
      } catch (e) {
        // Unreachable is normal (the cloud lives in a house); keep the last list.
        set({ cloudNotice: `Your cloud can't be reached right now. ${errorText(e)}` });
      }
    },

    async openCloudChat(cloudId) {
      const mine = get().conversations.find((c) => c.cloudId === cloudId);
      if (mine && spaceOf(mine.id) !== "local") return get().selectChat(mine.id);
      try {
        const id = await api.cloudImport(cloudId);
        const conversations = (await api.chatsList()).map(fromMeta);
        set({ conversations, cloudNotice: null });
        await get().selectChat(id);
      } catch (e) {
        set({ cloudNotice: `Couldn't open that chat: ${errorText(e)}` });
      }
    },

    async setWorkspace(workspace) {
      set({ currentId: null, pending: [], pendingFiles: [] });
      await get().updateSettings({ workspace, useCloud: workspace === "cloud" });
      if (workspace !== "local") void get().refreshCloudChats();
    },

    keepAnswer(msgId) {
      const conv = get().conversations.find((c) => c.messages.some((m) => m.id === msgId));
      const group = conv?.messages.find((m) => m.id === msgId)?.group;
      if (!conv || !group) return;
      patchConversation(conv.id, (c) => ({
        ...c,
        messages: c.messages.map((m) => (m.group === group ? { ...m, alt: m.id !== msgId, kept: m.id === msgId || undefined } : m)),
      }));
      const done = get().conversations.find((c) => c.id === conv.id);
      if (done && inTauri && !done.private) scheduleSave(done, 100);
      if (!inTauri) saveConversationsLocally(get().conversations);
    },

    async cloudAct(msgId, action, value) {
      const conv = currentConversation(get());
      const msg = conv?.messages.find((m) => m.id === msgId);
      if (!conv?.cloudId || !msg?.remoteId) return;
      const base = { requestId: uid(), conversationId: conv.cloudId, messageId: msg.remoteId };
      if (action === "feedback") {
        patchMessage(conv.id, msgId, (m) => ({ ...m, feedback: value }));
        await api.cloudAction({ ...base, action, value }, () => {}).catch((e) => console.warn("feedback not sent", e));
        return;
      }
      if (action === "answer-now") {
        await api.cloudAction({ ...base, action }, () => {}).catch((e) => console.warn("answer-now failed", e));
        return;
      }
      if (get().generating) return;
      // Deepen / justify write a new answer after this one.
      const reply: Message = {
        id: uid(),
        role: "assistant",
        content: "",
        status: "streaming",
        cloud: true,
        cloudMode: msg.cloudMode,
        createdAt: Date.now(),
      };
      patchConversation(conv.id, (c) => {
        const i = c.messages.findIndex((m) => m.id === msgId);
        return { ...c, messages: [...c.messages.slice(0, i + 1), reply, ...c.messages.slice(i + 1)] };
      });
      await streamReply(conv.id, { ...reply, id: reply.id }, (onEvent) =>
        api.cloudAction({ ...base, requestId: reply.id, action, since: msg.remoteId }, onEvent),
      );
    },

    async attachFiles(paths) {
      let convId = get().currentId;
      if (!convId || !get().conversations.some((c) => c.id === convId)) {
        const conv: Conversation = { id: newId(workspaceOf(get().settings)), title: "New chat", createdAt: Date.now(), updatedAt: Date.now(), messages: [], loaded: true };
        set({ conversations: [conv, ...get().conversations], currentId: conv.id });
        convId = conv.id;
      }
      set({ attaching: get().attaching + paths.length, attachError: null });
      for (const path of paths) {
        const name = path.split(/[\\/]/).pop() ?? "file";
        try {
          const conv = get().conversations.find((c) => c.id === convId)!;
          const res = await api.cloudAttach(conv.cloudId ?? null, conv.messages[0]?.content ?? name, path);
          set({
            conversations: get().conversations.map((c) => (c.id === convId ? { ...c, cloudId: res.conversationId } : c)),
          });
          const id = idOf(res.attachment);
          if (id) set({ pending: [...get().pending, { id, name, image: isImage({ ...res.attachment, filename: res.attachment.filename ?? name }) }] });
        } catch (e) {
          set({ attachError: `${name}: ${errorText(e)}` });
        } finally {
          set({ attaching: Math.max(0, get().attaching - 1) });
        }
      }
    },

    async attachLocal(paths) {
      set({ attaching: get().attaching + paths.length, attachError: null });
      for (const path of paths) {
        const name = path.split(/[\\/]/).pop() ?? "file";
        try {
          const file = await api.fileIngest(path);
          const engine = get().engine;
          if (file.kind === "image" && !(engine.state === "ready" && engine.vision)) {
            set({ attachError: `${name}: the loaded model can't see photos. Load a model marked "Sees images" (Models).` });
            continue;
          }
          set({ pendingFiles: [...get().pendingFiles, file] });
        } catch (e) {
          set({ attachError: `${name}: ${errorText(e)}` });
        } finally {
          set({ attaching: Math.max(0, get().attaching - 1) });
        }
      }
    },

    removePendingFile(index) {
      set({ pendingFiles: get().pendingFiles.filter((_, i) => i !== index) });
    },

    async loadSavedPrompts(force = false) {
      if (!inTauri || !get().settings?.cloudConnected || (get().savedPrompts && !force)) return;
      try {
        const rows = listOf(await api.cloudGet("/api/saved-prompts"));
        set({
          savedPrompts: rows
            .map((r) => ({ id: idOf(r) ?? "", title: titleOf(r), text: str(r.content) ?? str(r.prompt) ?? str(r.text) ?? "" }))
            .filter((p) => p.text),
        });
      } catch (e) {
        console.warn("saved prompts unavailable", e);
      }
    },

    attachExisting(a) {
      if (!get().pending.some((p) => p.id === a.id)) set({ pending: [...get().pending, a] });
    },

    removePending(id) {
      set({ pending: get().pending.filter((p) => p.id !== id) });
    },

    async refreshKb() {
      if (!inTauri) return;
      try {
        set({ kb: await api.kbStatus() });
      } catch (e) {
        console.warn("knowledge base status unavailable", e);
      }
    },

    toggleFiles() {
      const s = get().settings;
      if (s) void get().updateSettings({ kbEnabled: !s.kbEnabled });
    },

    toggleWeb() {
      const s = get().settings;
      if (s) void get().updateSettings(nextWeb(webState(s)));
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
    // The main model's image reader just arrived: restart so it loads with it.
    const id = e.id.endsWith(":vision") ? e.id.slice(0, -":vision".length) : null;
    if (e.kind === "finished" && id && get().settings?.activeModel?.startsWith(`${id}:`)) void api.engineRestart().catch(() => undefined);
    // The search-by-meaning model arrived: prepare the passages already indexed.
    if (e.kind === "finished" && e.id === get().kb?.embedKey) {
      void get().refreshKb();
      if (get().kb?.sources.length) void api.kbReindex().catch(() => undefined);
    }
  }
});

/** macOS voices read answers aloud (`say`); elsewhere there's no speech yet. */
/** The app is running on a Mac. Mac-only features (AppleScript actions, Upkeep, Messages) key off this. */
export const onMac = () => inTauri && /Mac/i.test(navigator.userAgent);
/** BYTE can speak aloud here: on a Mac, and on Windows now that its audio backend exists. Not the same
 *  question as onMac: callers used canSpeak() as a stand-in for "is a Mac", which would have shown the
 *  Mac-only example prompts on Windows the moment voice was enabled there. */
export const canSpeak = () => onMac() || (inTauri && isWindows());

export const currentConversation = (s: State) => s.conversations.find((c) => c.id === s.currentId) ?? null;
