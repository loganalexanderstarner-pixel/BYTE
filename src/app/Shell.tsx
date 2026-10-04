import {
  BookOpen,
  GraduationCap,
  FileText,
  PanelLeft,
  Settings as SettingsIcon,
  SquarePen,
  PenLine,
  Briefcase,
  Bot,
  ClipboardList,
  ListTodo,
  NotebookPen,
  LifeBuoy,
  Shapes,
  WifiOff,
  KeyRound,
  MessageCircle,
} from "lucide-react";
import { lazy, Suspense, useEffect, useMemo, useState } from "react";

import { ChatView } from "../components/chat/ChatView";
import { Composer } from "../components/chat/Composer";
import { EngineBadge } from "../components/EngineBadge";
import { api } from "../lib/api";
import { Palette } from "../components/Palette";
import { THEMES } from "../design/themes";
import { prettyKeys } from "../lib/keys";
import type { PaletteItem } from "../lib/palette";
import type { NewText } from "../lib/types";
import { Sidebar } from "../components/Sidebar";
import { KidsExit } from "../components/kids/KidsExit";
import { useStore, type SettingsTab } from "../state/store";
import { TABS } from "../components/settings/tabs";

// Panels opened on demand load on first use, so BYTE starts with less code to read.
const DocumentsPanel = lazy(() =>
  import("../components/documents/DocumentsPanel").then((m) => ({
    default: m.DocumentsPanel,
  })),
);
const RecipeBox = lazy(() =>
  import("../components/kitchen/RecipeBox").then((m) => ({
    default: m.RecipeBox,
  })),
);
const StudyPanel = lazy(() =>
  import("../components/study/StudyPanel").then((m) => ({
    default: m.StudyPanel,
  })),
);
const WritingPanel = lazy(() =>
  import("../components/writing/WritingPanel").then((m) => ({
    default: m.WritingPanel,
  })),
);
const MindMapView = lazy(() =>
  import("../components/notes/MindMapView").then((m) => ({
    default: m.MindMapView,
  })),
);
const NotesPanel = lazy(() =>
  import("../components/notes/NotesPanel").then((m) => ({
    default: m.NotesPanel,
  })),
);
const HelpCenter = lazy(() =>
  import("../components/help/HelpCenter").then((m) => ({
    default: m.HelpCenter,
  })),
);
const BoardPanel = lazy(() =>
  import("../components/board/BoardPanel").then((m) => ({
    default: m.BoardPanel,
  })),
);
const JobsPanel = lazy(() =>
  import("../components/jobs/JobsPanel").then((m) => ({
    default: m.JobsPanel,
  })),
);
const MessagesPanel = lazy(() =>
  import("../components/messages/MessagesPanel").then((m) => ({
    default: m.MessagesPanel,
  })),
);
const ClipboardPanel = lazy(() =>
  import("../components/clipboard/ClipboardPanel").then((m) => ({
    default: m.ClipboardPanel,
  })),
);
const TasksPanel = lazy(() =>
  import("../components/tasks/TasksPanel").then((m) => ({
    default: m.TasksPanel,
  })),
);
const AssistantsPanel = lazy(() =>
  import("../components/assistants/AssistantsPanel").then((m) => ({
    default: m.AssistantsPanel,
  })),
);
const Reader = lazy(() =>
  import("../components/reader/Reader").then((m) => ({ default: m.Reader })),
);
const SettingsModal = lazy(() =>
  import("../components/settings/SettingsModal").then((m) => ({
    default: m.SettingsModal,
  })),
);

export function Shell() {
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const newChat = useStore((s) => s.newChat);
  const openSettings = useStore((s) => s.openSettings);
  const cloudConnected = useStore((s) => !!s.settings?.cloudConnected);
  const [docsOpen, setDocsOpen] = useState(false);
  const [recipesOpen, setRecipesOpen] = useState(false);
  const kitchenOn = useStore((s) => s.settings?.kitchenEnabled ?? true);
  const studyOn = useStore((s) => s.settings?.studyEnabled !== false);
  const studyOpen = useStore((s) => !!s.study);
  const openStudy = useStore((s) => s.openStudy);
  const writingOn = useStore((s) => s.settings?.writingEnabled !== false);
  const writingOpen = useStore((s) => !!s.writing);
  const openWriting = useStore((s) => s.openWriting);
  const notesOn = useStore((s) => s.settings?.notesEnabled !== false);
  const notesOpen = useStore((s) => !!s.notes);
  const notesKey = useStore((s) => s.notes?.seq ?? 0);
  const openNotes = useStore((s) => s.openNotes);
  const mindmapOpen = useStore((s) => !!s.mindmap);
  const helpOpen = useStore((s) => !!s.help);
  const boardKey = useStore((s) => s.board?.seq ?? 0);
  const boardOpen = useStore((s) => !!s.board);
  const jobsOn = useStore((s) => s.settings?.jobsEnabled !== false);
  const [jobsOpen, setJobsOpen] = useState(false);
  const assistantsOn = useStore((s) => s.settings?.assistantsEnabled !== false);
  const [assistantsOpen, setAssistantsOpen] = useState(false);
  const clipsOn = useStore(
    (s) =>
      s.settings?.macControl !== false && s.settings?.clipboardHistory === true,
  );
  const [clipsOpen, setClipsOpen] = useState(false);
  // The Messages inbox (macOS, opt-in), and the newest text that arrived while BYTE is open.
  const messagesOn = useStore(
    (s) =>
      s.settings?.macControl !== false && s.settings?.messagesInbox === true,
  );
  const [messagesOpen, setMessagesOpen] = useState<{
    chat: string | null;
    draft: boolean;
  } | null>(null);
  const [newText, setNewText] = useState<NewText | null>(null);
  const tasksOn = useStore(
    (s) =>
      s.settings?.tasksEnabled !== false ||
      s.settings?.watchEnabled !== false ||
      s.settings?.automationsEnabled !== false ||
      s.settings?.trackersEnabled !== false,
  );
  const [tasksOpen, setTasksOpen] = useState(false);
  const reloadChats = useStore((s) => s.reloadChats);
  const addNewChats = useStore((s) => s.addNewChats);
  const openFresh = useStore((s) => s.openFresh);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const kids = useStore((s) => !!s.settings?.kidsMode);
  const [kidsExit, setKidsExit] = useState(false);

  // Quick Ask: its chats join the list, and "Open in BYTE" opens one here.
  useEffect(() => {
    const offs = [
      api.onQuickSaved(() => void addNewChats()),
      api.onQuickOpen((id) => (id ? void openFresh(id) : undefined)),
    ];
    return () => offs.forEach((p) => void p.then((off) => off()));
  }, [addNewChats, openFresh]);

  // The menu-bar icon's Offline tick, and its "Lock BYTE" when the lock is off (opens Settings → Privacy).
  useEffect(() => {
    const offs = [
      api.onOfflineChanged(
        () =>
          void api
            .settingsGet()
            .then((settings) => useStore.setState({ settings })),
      ),
      api.onOpenSettings((tab) => openSettings(tab as SettingsTab)),
    ];
    return () => offs.forEach((p) => void p.then((off) => off()));
  }, [openSettings]);

  useEffect(() => {
    const off = api.onNewTexts((t) => t.length && setNewText(t[t.length - 1]));
    return () => void off.then((f) => f());
  }, []);

  // A newer BYTE is out (the daily check): a quiet banner with Install.
  const [newVersion, setNewVersion] = useState<string | null>(null);
  useEffect(() => {
    const off = api.onUpdateAvailable((u) => setNewVersion(u.version));
    return () => void off.then((f) => f());
  }, []);

  // A scheduled run (briefing, scheduled question) saved a new chat: show it in the list.
  useEffect(() => {
    const off = api.onScheduleRan(() => void reloadChats());
    return () => void off.then((f) => f());
  }, [reloadChats]);
  const writingKey = useStore((s) => s.writing?.seq ?? 0);
  const settingsTab = useStore((s) => s.settingsTab);
  const stop = useStore((s) => s.stop);
  const reading = useStore((s) => !!s.reader);

  // ⌥⌘B in another app: its selected text opens in the writing studio.
  useEffect(() => {
    const offs = [
      api.onSelection((c) => openWriting(c.text, c.app || undefined)),
      api.onSelectionError((m) => openWriting("", undefined, m)),
    ];
    return () => offs.forEach((p) => void p.then((off) => off()));
  }, [openWriting]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      // Kids mode: no palette, settings or panels from the keyboard.
      if (
        useStore.getState().settings?.kidsMode &&
        cmd &&
        ["k", ",", "?", "/"].includes(e.key.toLowerCase())
      ) {
        e.preventDefault();
        return;
      }
      if (cmd && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      } else if (cmd && e.key.toLowerCase() === "n") {
        e.preventDefault();
        newChat();
      } else if (cmd && e.key === ",") {
        e.preventDefault();
        openSettings("models");
      } else if (cmd && (e.key === "?" || (e.key === "/" && e.shiftKey))) {
        e.preventDefault();
        useStore.getState().openHelp();
      } else if (cmd && e.key === "\\") {
        e.preventDefault();
        toggleSidebar();
      } else if (e.key === "Escape" && !useStore.getState().settingsTab) {
        void stop();
        // Esc also ends hands-free talking and stops BYTE reading aloud.
        useStore.getState().setTalk(false);
        useStore.getState().stopSpeaking();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [newChat, openSettings, toggleSidebar, stop]);

  const conversations = useStore((s) => s.conversations);
  const offline = useStore((s) => !!s.settings?.offline);
  const lockOn = useStore((s) => !!s.settings?.lockEnabled);
  const quickKeys = useStore((s) =>
    s.settings?.quickAsk !== false
      ? (s.settings?.quickAskKeys ?? "Alt+Space")
      : null,
  );
  const paletteItems = useMemo(() => {
    if (!paletteOpen) return [];
    const items: PaletteItem[] = [
      { id: "new", label: "New chat", hint: "⌘N", group: "Actions" },
      {
        id: "private",
        label: "New private chat",
        keywords: "incognito secret",
        group: "Actions",
      },
      {
        id: "sidebar",
        label: "Show or hide the sidebar",
        hint: "⌘\\",
        group: "Actions",
      },
      {
        id: "web",
        label: "Web search: switch Off / Auto / Always",
        keywords: "internet online",
        group: "Actions",
      },
      {
        id: "offline",
        label: offline
          ? "Go back online"
          : "Go offline (nothing reaches the internet)",
        keywords: "offline airplane privacy internet network",
        group: "Actions",
      },
      {
        id: "docs",
        label: "Documents (beta): PDFs, slides and Word files",
        keywords: "pdf pptx docx report deck",
        group: "Actions",
      },
    ];
    if (lockOn)
      items.push({
        id: "lock",
        label: "Lock BYTE now",
        keywords: "touch id privacy password",
        group: "Actions",
      });
    if (quickKeys)
      items.push({
        id: "quick",
        label: "Quick Ask window",
        hint: prettyKeys(quickKeys),
        group: "Actions",
      });
    if (kitchenOn)
      items.push({
        id: "recipes",
        label: "Recipe box",
        keywords: "kitchen cooking",
        group: "Actions",
      });
    if (assistantsOn)
      items.push({ id: "assistants", label: "Assistants", group: "Actions" });
    if (tasksOn)
      items.push({
        id: "tasks",
        label: "Tasks, schedules, trackers and automations",
        keywords: "to-do todo reminders bills feeds watchers",
        group: "Actions",
      });
    if (clipsOn)
      items.push({ id: "clips", label: "Clipboard history", group: "Actions" });
    if (messagesOn)
      items.push({
        id: "messages",
        label: "Messages: your texts",
        keywords: "imessage sms reply inbox",
        group: "Actions",
      });
    if (jobsOn)
      items.push({ id: "jobs", label: "Job search", group: "Actions" });
    if (writingOn)
      items.push({
        id: "writing",
        label: "Writing studio",
        keywords: "rewrite grammar tone",
        group: "Actions",
      });
    items.push({
      id: "board",
      label: "Brainstorm board",
      keywords: "ideas sticky notes canvas",
      group: "Actions",
    });
    items.push({
      id: "help",
      label: "Help",
      hint: "⌘?",
      keywords: "how to guide support faq",
      group: "Actions",
    });
    items.push({
      id: "ideas",
      label: "Ideas to try",
      keywords: "examples prompts what can byte do",
      group: "Actions",
    });
    if (notesOn)
      items.push({
        id: "notes",
        label: "Notes",
        keywords: "notebook markdown clips",
        group: "Actions",
      });
    if (notesOn)
      items.push({
        id: "note-new",
        label: "New note",
        keywords: "write jot",
        group: "Actions",
      });
    if (studyOn)
      items.push({
        id: "study",
        label: "Study: flashcard decks",
        keywords: "flashcards quiz learn",
        group: "Actions",
      });
    for (const t of TABS)
      items.push({
        id: `settings:${t.id}`,
        label: `Settings: ${t.label}`,
        hint: t.id === "models" ? "⌘," : undefined,
        keywords: "preferences options",
        group: "Settings",
      });
    for (const m of ["fast", "auto", "deep", "extended"] as const)
      items.push({
        id: `mode:${m}`,
        label: `Mode: ${m[0].toUpperCase()}${m.slice(1)}`,
        group: "Modes",
      });
    for (const t of THEMES)
      items.push({
        id: `theme:${t.id}`,
        label: `Theme: ${t.name}`,
        keywords: "colors appearance",
        group: "Themes",
      });
    for (const c of conversations)
      if (!c.private && c.messages.length + (c.messageCount ?? 0) > 0)
        items.push({
          id: `chat:${c.id}`,
          label: c.title || "Untitled chat",
          keywords: c.summary ?? undefined,
          group: "Chats",
        });
    return items;
  }, [
    paletteOpen,
    offline,
    lockOn,
    conversations,
    quickKeys,
    kitchenOn,
    assistantsOn,
    tasksOn,
    clipsOn,
    jobsOn,
    writingOn,
    studyOn,
    notesOn,
  ]);

  const runItem = (it: PaletteItem) => {
    const [kind, arg] = it.id.includes(":")
      ? [
          it.id.slice(0, it.id.indexOf(":")),
          it.id.slice(it.id.indexOf(":") + 1),
        ]
      : [it.id, ""];
    const st = useStore.getState();
    switch (kind) {
      case "new":
        return newChat();
      case "private":
        return newChat(true);
      case "sidebar":
        return toggleSidebar();
      case "web":
        return st.toggleWeb();
      case "offline":
        return void st.updateSettings({ offline: !offline });
      case "lock":
        return void api.lockNow();
      case "docs":
        return setDocsOpen(true);
      case "quick":
        return void api.quickToggle();
      case "recipes":
        return setRecipesOpen(true);
      case "assistants":
        return setAssistantsOpen(true);
      case "tasks":
        return setTasksOpen(true);
      case "clips":
        return setClipsOpen(true);
      case "messages":
        return setMessagesOpen({ chat: null, draft: false });
      case "jobs":
        return setJobsOpen(true);
      case "writing":
        return openWriting();
      case "notes":
        return openNotes();
      case "help":
        return useStore.getState().openHelp();
      case "board":
        return useStore.getState().openBoard();
      case "ideas":
        return useStore.getState().openHelp("ideas");
      case "note-new":
        return openNotes({
          draft: { title: "", folder: "Inbox", tags: [], body: "" },
        });
      case "study":
        return openStudy();
      case "settings":
        return openSettings(arg as (typeof TABS)[number]["id"]);
      case "mode":
        return st.setMode(arg as "fast" | "auto" | "deep" | "extended");
      case "theme":
        return void st.updateSettings({ theme: arg });
      case "chat":
        return void st.selectChat(arg);
    }
  };

  return (
    <div
      className={`app ${sidebarOpen ? "" : "sidebar-hidden"} ${reading ? "reading" : ""} ${kids ? "kids" : ""}`}
    >
      <Sidebar />
      <main className="main">
        <header className="titlebar" data-tauri-drag-region>
          <div className="row">
            {!sidebarOpen && (
              <>
                <button
                  className="icon-btn"
                  onClick={toggleSidebar}
                  title="Show sidebar (⌘\)"
                >
                  <PanelLeft size={18} />
                </button>
                <button
                  className="icon-btn"
                  onClick={() => newChat()}
                  title="New chat (⌘N)"
                >
                  <SquarePen size={18} />
                </button>
              </>
            )}
          </div>
          {kids ? (
            <div className="row no-drag">
              <span className="kids-badge">Kids mode</span>
              <button
                className="btn sm ghost"
                onClick={() => setKidsExit(true)}
                title="Turn off kids mode (needs the PIN)"
              >
                <KeyRound size={14} /> Grown-ups
              </button>
            </div>
          ) : (
            <div className="row no-drag">
              {offline && (
                <button
                  className="offline-pill"
                  onClick={() =>
                    void useStore.getState().updateSettings({ offline: false })
                  }
                  title="BYTE is offline: nothing reaches the internet. Click to go back online."
                >
                  <WifiOff size={13} /> Offline
                </button>
              )}
              <EngineBadge />
              <button
                className="icon-btn"
                onClick={() => setDocsOpen(true)}
                title={
                  cloudConnected
                    ? "Documents (beta): PDFs, slides and Word files, made on your BYTE cloud or this Mac"
                    : "Documents (beta): PDFs, slides and Word files, made on this Mac"
                }
              >
                <FileText size={18} />
              </button>
              {kitchenOn && (
                <button
                  className="icon-btn"
                  onClick={() => setRecipesOpen(true)}
                  title="Recipe box: your saved recipes"
                >
                  <BookOpen size={18} />
                </button>
              )}
              {assistantsOn && (
                <button
                  className="icon-btn"
                  onClick={() => setAssistantsOpen(true)}
                  title="Assistants: BYTE set up for one job"
                >
                  <Bot size={18} />
                </button>
              )}
              {tasksOn && (
                <button
                  className="icon-btn"
                  onClick={() => setTasksOpen(true)}
                  title="Tasks: your to-do list, schedules, trackers, automations, news feeds and watched pages"
                >
                  <ListTodo size={18} />
                </button>
              )}
              {messagesOn && !kids && (
                <button
                  className="icon-btn"
                  onClick={() => setMessagesOpen({ chat: null, draft: false })}
                  title="Messages: your texts, with replies BYTE can draft"
                >
                  <MessageCircle size={18} />
                </button>
              )}
              {clipsOn && (
                <button
                  className="icon-btn"
                  onClick={() => setClipsOpen(true)}
                  title="Clipboard history: what you copied lately"
                >
                  <ClipboardList size={18} />
                </button>
              )}
              {jobsOn && (
                <button
                  className="icon-btn"
                  onClick={() => setJobsOpen(true)}
                  title="Job search: postings, deadlines, interview prep"
                >
                  <Briefcase size={18} />
                </button>
              )}
              {writingOn && (
                <button
                  className="icon-btn"
                  onClick={() => openWriting()}
                  title="Writing studio: rewrite, shorten, expand, tone, grammar"
                >
                  <PenLine size={18} />
                </button>
              )}
              {notesOn && (
                <button
                  className="icon-btn"
                  onClick={() => openNotes()}
                  title="Notes: your Markdown notes and web clips"
                >
                  <NotebookPen size={18} />
                </button>
              )}
              {studyOn && (
                <button
                  className="icon-btn"
                  onClick={() => openStudy()}
                  title="Study: your flashcard decks"
                >
                  <GraduationCap size={18} />
                </button>
              )}
              <button
                className="icon-btn"
                onClick={() => useStore.getState().openBoard()}
                title="Brainstorm board: sticky notes BYTE can add ideas to"
              >
                <Shapes size={18} />
              </button>
              <button
                className="icon-btn"
                onClick={() => useStore.getState().openHelp()}
                title="Help (⌘?)"
              >
                <LifeBuoy size={18} />
              </button>
              <button
                className="icon-btn"
                onClick={() => openSettings("models")}
                title="Settings (⌘,)"
              >
                <SettingsIcon size={18} />
              </button>
            </div>
          )}
        </header>
        {newText && messagesOn && !kids && (
          <div className="banner update-banner" role="status">
            <span className="grow ellipsis">
              <b>{newText.name}:</b> {newText.text}
            </span>
            <button
              className="btn sm primary"
              onClick={() => (
                setMessagesOpen({ chat: newText.chat, draft: true }),
                setNewText(null)
              )}
            >
              Draft a reply
            </button>
            <button
              className="btn sm"
              onClick={() => (
                setMessagesOpen({ chat: newText.chat, draft: false }),
                setNewText(null)
              )}
            >
              Reply
            </button>
            <button
              className="icon-btn sm"
              onClick={() => setNewText(null)}
              aria-label="Dismiss"
            >
              ×
            </button>
          </div>
        )}
        {newVersion && !kids && (
          <div className="banner update-banner" role="status">
            <span className="grow">BYTE {newVersion} is out.</span>
            <button
              className="btn sm primary"
              onClick={() => openSettings("about")}
            >
              See what's new
            </button>
            <button
              className="icon-btn sm"
              onClick={() => setNewVersion(null)}
              aria-label="Dismiss"
            >
              ×
            </button>
          </div>
        )}
        <ChatView />
        <Composer />
      </main>
      <Suspense fallback={null}>
        {reading && <Reader />}
        {settingsTab && <SettingsModal />}
        {docsOpen && <DocumentsPanel onClose={() => setDocsOpen(false)} />}
        {recipesOpen && <RecipeBox onClose={() => setRecipesOpen(false)} />}
        {studyOpen && <StudyPanel />}
        {writingOpen && <WritingPanel key={writingKey} />}
        {notesOpen && <NotesPanel key={notesKey} />}
        {mindmapOpen && <MindMapView />}
        {helpOpen && <HelpCenter />}
        {boardOpen && <BoardPanel key={boardKey} />}
        {clipsOpen && <ClipboardPanel onClose={() => setClipsOpen(false)} />}
        {messagesOpen && (
          <MessagesPanel
            open={messagesOpen.chat}
            draft={messagesOpen.draft}
            onClose={() => setMessagesOpen(null)}
          />
        )}
        {tasksOpen && <TasksPanel onClose={() => setTasksOpen(false)} />}
        {jobsOpen && <JobsPanel onClose={() => setJobsOpen(false)} />}
        {assistantsOpen && (
          <AssistantsPanel onClose={() => setAssistantsOpen(false)} />
        )}
      </Suspense>
      {kidsExit && <KidsExit onClose={() => setKidsExit(false)} />}
      {paletteOpen && !kids && (
        <Palette
          items={paletteItems}
          onRun={runItem}
          onClose={() => setPaletteOpen(false)}
        />
      )}
    </div>
  );
}
