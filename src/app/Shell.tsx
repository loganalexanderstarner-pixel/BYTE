import { BookOpen, GraduationCap, FileText, PanelLeft, Settings as SettingsIcon, SquarePen, PenLine, Briefcase, Bot, ClipboardList, ListTodo } from "lucide-react";
import { useEffect, useState } from "react";

import { ChatView } from "../components/chat/ChatView";
import { Composer } from "../components/chat/Composer";
import { EngineBadge } from "../components/EngineBadge";
import { DocumentsPanel } from "../components/documents/DocumentsPanel";
import { RecipeBox } from "../components/kitchen/RecipeBox";
import { StudyPanel } from "../components/study/StudyPanel";
import { WritingPanel } from "../components/writing/WritingPanel";
import { JobsPanel } from "../components/jobs/JobsPanel";
import { ClipboardPanel } from "../components/clipboard/ClipboardPanel";
import { TasksPanel } from "../components/tasks/TasksPanel";
import { api } from "../lib/api";
import { AssistantsPanel } from "../components/assistants/AssistantsPanel";
import { Reader } from "../components/reader/Reader";
import { SettingsModal } from "../components/settings/SettingsModal";
import { Sidebar } from "../components/Sidebar";
import { useStore } from "../state/store";

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
  const jobsOn = useStore((s) => s.settings?.jobsEnabled !== false);
  const [jobsOpen, setJobsOpen] = useState(false);
  const assistantsOn = useStore((s) => s.settings?.assistantsEnabled !== false);
  const [assistantsOpen, setAssistantsOpen] = useState(false);
  const clipsOn = useStore((s) => s.settings?.macControl !== false && s.settings?.clipboardHistory === true);
  const [clipsOpen, setClipsOpen] = useState(false);
  const tasksOn = useStore((s) => s.settings?.tasksEnabled !== false);
  const [tasksOpen, setTasksOpen] = useState(false);
  const reloadChats = useStore((s) => s.reloadChats);

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
      if (cmd && e.key.toLowerCase() === "n") {
        e.preventDefault();
        newChat();
      } else if (cmd && e.key === ",") {
        e.preventDefault();
        openSettings("models");
      } else if (cmd && e.key === "\\") {
        e.preventDefault();
        toggleSidebar();
      } else if (e.key === "Escape" && !useStore.getState().settingsTab) {
        void stop();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [newChat, openSettings, toggleSidebar, stop]);

  return (
    <div className={`app ${sidebarOpen ? "" : "sidebar-hidden"} ${reading ? "reading" : ""}`}>
      <Sidebar />
      <main className="main">
        <header className="titlebar" data-tauri-drag-region>
          <div className="row">
            {!sidebarOpen && (
              <>
                <button className="icon-btn" onClick={toggleSidebar} title="Show sidebar (⌘\)">
                  <PanelLeft size={18} />
                </button>
                <button className="icon-btn" onClick={() => newChat()} title="New chat (⌘N)">
                  <SquarePen size={18} />
                </button>
              </>
            )}
          </div>
          <div className="row no-drag">
            <EngineBadge />
            <button
              className="icon-btn"
              onClick={() => setDocsOpen(true)}
              title={cloudConnected ? "Documents: PDFs, slides and Word files, made on your BYTE cloud or this Mac" : "Documents: PDFs, slides and Word files, made on this Mac"}
            >
              <FileText size={18} />
            </button>
            {kitchenOn && (
              <button className="icon-btn" onClick={() => setRecipesOpen(true)} title="Recipe box: your saved recipes">
                <BookOpen size={18} />
              </button>
            )}
            {assistantsOn && (
              <button className="icon-btn" onClick={() => setAssistantsOpen(true)} title="Assistants: BYTE set up for one job">
                <Bot size={18} />
              </button>
            )}
            {tasksOn && (
              <button className="icon-btn" onClick={() => setTasksOpen(true)} title="Tasks: your to-do list and what BYTE does on a schedule">
                <ListTodo size={18} />
              </button>
            )}
            {clipsOn && (
              <button className="icon-btn" onClick={() => setClipsOpen(true)} title="Clipboard history: what you copied lately">
                <ClipboardList size={18} />
              </button>
            )}
            {jobsOn && (
              <button className="icon-btn" onClick={() => setJobsOpen(true)} title="Job search: postings, deadlines, interview prep">
                <Briefcase size={18} />
              </button>
            )}
            {writingOn && (
              <button className="icon-btn" onClick={() => openWriting()} title="Writing studio: rewrite, shorten, expand, tone, grammar">
                <PenLine size={18} />
              </button>
            )}
            {studyOn && (
              <button className="icon-btn" onClick={() => openStudy()} title="Study: your flashcard decks">
                <GraduationCap size={18} />
              </button>
            )}
            <button className="icon-btn" onClick={() => openSettings("models")} title="Settings (⌘,)">
              <SettingsIcon size={18} />
            </button>
          </div>
        </header>
        <ChatView />
        <Composer />
      </main>
      {reading && <Reader />}
      {settingsTab && <SettingsModal />}
      {docsOpen && <DocumentsPanel onClose={() => setDocsOpen(false)} />}
      {recipesOpen && <RecipeBox onClose={() => setRecipesOpen(false)} />}
      {studyOpen && <StudyPanel />}
      {writingOpen && <WritingPanel key={writingKey} />}
      {clipsOpen && <ClipboardPanel onClose={() => setClipsOpen(false)} />}
      {tasksOpen && <TasksPanel onClose={() => setTasksOpen(false)} />}
      {jobsOpen && <JobsPanel onClose={() => setJobsOpen(false)} />}
      {assistantsOpen && <AssistantsPanel onClose={() => setAssistantsOpen(false)} />}
    </div>
  );
}
