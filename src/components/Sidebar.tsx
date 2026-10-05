import { ask } from "@tauri-apps/plugin-dialog";
import {
  Briefcase,
  ChevronRight,
  Cloud,
  Laptop,
  RefreshCw,
  Sparkles,
  EyeOff,
  Folder,
  FolderInput,
  FolderPlus,
  Lock,
  MessageSquare,
  MoreHorizontal,
  PanelLeft,
  Pencil,
  Pin,
  PinOff,
  Search,
  SquarePen,
  Plus,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { Logo } from "../design/Logo";
import { api, inTauri } from "../lib/api";
import type { Project, SearchHit, Workspace } from "../lib/types";
import { listSignature } from "../lib/throttle";
import { hasMessages, spaceOf, useStore, workspaceOf, type CloudChat, type Conversation } from "../state/store";
import { platformKeys as K } from "../lib/keys";

function groupLabel(ts: number): string {
  const day = 86_400_000;
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  const t = start.getTime();
  if (ts >= t) return "Today";
  if (ts >= t - day) return "Yesterday";
  if (ts >= t - 7 * day) return "Previous 7 days";
  if (ts >= t - 30 * day) return "Previous 30 days";
  return "Older";
}

/** Sidebar sections: pinned, projects, folders, then the rest by date. */
export function sidebarSections(conversations: Conversation[], projectList: Project[] = []) {
  const visible = conversations.filter(hasMessages);
  const pinned = visible.filter((c) => c.pinned);
  const folders = new Map<string, Conversation[]>();
  const dated: { label: string; items: Conversation[] }[] = [];
  const byProject = new Map(projectList.map((p) => [p.id, [] as Conversation[]]));
  for (const c of visible) {
    if (c.pinned) continue;
    if (c.projectId && byProject.has(c.projectId)) {
      byProject.get(c.projectId)!.push(c);
      continue;
    }
    if (c.folder) {
      folders.set(c.folder, [...(folders.get(c.folder) ?? []), c]);
      continue;
    }
    const label = c.private ? "Private (not saved)" : groupLabel(c.updatedAt);
    const g = dated.find((x) => x.label === label);
    if (g) g.items.push(c);
    else dated.push({ label, items: [c] });
  }
  const byRecent = (a: Conversation, b: Conversation) => b.updatedAt - a.updatedAt;
  return {
    pinned: pinned.sort(byRecent),
    projects: projectList.map((p) => ({ project: p, items: byProject.get(p.id)!.sort(byRecent) })),
    folders: [...folders.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([name, items]) => ({ name, items: items.sort(byRecent) })),
    dated,
  };
}

/** Chats that belong to a workspace (private chats only ever live on this Mac). */
export function inWorkspace(conversations: Conversation[], ws: Workspace): Conversation[] {
  return conversations.filter((c) => (c.private ? ws === "local" : spaceOf(c.id) === ws));
}

/** Cloud conversations with no copy on this Mac yet. */
export function onlyOnCloud(cloudChats: CloudChat[] | null, conversations: Conversation[]): CloudChat[] {
  const mirrored = new Set(conversations.filter((c) => c.cloudId && spaceOf(c.id) === "cloud").map((c) => c.cloudId));
  return (cloudChats ?? []).filter((c) => !mirrored.has(c.id));
}

const WORKSPACES: { id: Workspace; label: string; hint: string; icon: typeof Cloud }[] = [
  { id: "local", label: "This Mac", hint: "Chats answered on this Mac", icon: Laptop },
  { id: "cloud", label: "Cloud", hint: "Chats on your BYTE cloud", icon: Cloud },
  { id: "both", label: "Both", hint: "Each question goes to this Mac and the cloud at once; keep the better answer", icon: Sparkles },
];

/** "«match» in context" → text with <mark>s. */
function Snippet({ text }: { text: string }) {
  const parts = text.split(/(«[^»]*»)/g);
  return (
    <>
      {parts.map((p, i) => (p.startsWith("«") ? <mark key={i}>{p.slice(1, -1)}</mark> : <span key={i}>{p}</span>))}
    </>
  );
}

export function Sidebar() {
  // Re-render only when the list changes, not with every streamed word.
  const signature = useStore((s) => listSignature(s.conversations));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const allConversations = useMemo(() => useStore.getState().conversations, [signature]);
  const settings = useStore((s) => s.settings);
  const workspace = workspaceOf(settings);
  const setWorkspace = useStore((s) => s.setWorkspace);
  const cloudChats = useStore((s) => s.cloudChats);
  const cloudNotice = useStore((s) => s.cloudNotice);
  const refreshCloudChats = useStore((s) => s.refreshCloudChats);
  const conversations = useMemo(() => inWorkspace(allConversations, workspace), [allConversations, workspace]);
  const remoteOnly = useMemo(() => (workspace === "cloud" ? onlyOnCloud(cloudChats, allConversations) : []), [workspace, cloudChats, allConversations]);
  const currentId = useStore((s) => s.currentId);
  const selectChat = useStore((s) => s.selectChat);
  const newChat = useStore((s) => s.newChat);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const projects = useStore((s) => s.projects);
  const [editingProject, setEditingProject] = useState<Project | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [closed, setClosed] = useState<Set<string>>(new Set());

  // Full-text search over every message, debounced.
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setHits(null);
      return;
    }
    if (!inTauri) {
      const lower = q.toLowerCase();
      setHits(
        conversations
          .filter((c) => c.title.toLowerCase().includes(lower) || c.messages.some((m) => m.content.toLowerCase().includes(lower)))
          .map((c) => ({ conversationId: c.id, messageId: "", title: c.title, snippet: "", updatedAt: c.updatedAt })),
      );
      return;
    }
    const t = setTimeout(() => {
      api.chatsSearch(q).then(setHits).catch(() => setHits([]));
    }, 150);
    return () => clearTimeout(t);
  }, [query, conversations]);

  // The cloud's list is re-read when the workspace opens and when the window comes back to the front.
  useEffect(() => {
    if (workspace === "local") return;
    void refreshCloudChats();
    const onFocus = () => void refreshCloudChats();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [workspace, refreshCloudChats]);

  // Projects add instructions to this Mac's prompt, so they live in the This Mac workspace.
  const kids = useStore((s) => !!s.settings?.kidsMode);
  const shownProjects = workspace === "local" && !kids ? projects : [];
  const sections = useMemo(() => sidebarSections(conversations, shownProjects), [conversations, shownProjects]);
  const folderNames = sections.folders.map((f) => f.name);
  const empty = !sections.pinned.length && !sections.folders.length && !sections.dated.length && !shownProjects.length && !remoteOnly.length;

  const toggleFolder = (name: string) => {
    const next = new Set(closed);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    setClosed(next);
  };

  return (
    <aside className="sidebar" aria-label="Conversations">
      <div className="titlebar" data-tauri-drag-region>
        <button className="icon-btn" onClick={toggleSidebar} title={K("Hide sidebar (⌘\\)")}>
          <PanelLeft size={18} />
        </button>
        {workspace === "local" && !kids && (
          <button className="icon-btn" onClick={() => newChat(true)} title="New private chat: not saved, doesn't use memory">
            <EyeOff size={17} />
          </button>
        )}
        <button className="icon-btn" onClick={() => newChat()} title={K("New chat (⌘N)")}>
          <SquarePen size={18} />
        </button>
      </div>
      <div className="sidebar-brand">
        <Logo size={32} />
        <span className="wordmark">BYTE</span>
      </div>
      {settings?.cloudConnected && (
        <div className="sidebar-section">
          <div className="segmented workspace-switch" role="tablist" aria-label="Workspace">
            {WORKSPACES.map(({ id, label, hint, icon: Icon }) => (
              <button key={id} role="tab" aria-selected={workspace === id} title={hint} onClick={() => workspace !== id && void setWorkspace(id)}>
                <Icon size={13} /> {label}
              </button>
            ))}
          </div>
        </div>
      )}
      <div className="sidebar-section">
        <label className="sidebar-search">
          <Search size={15} className="faint" />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search all chats" aria-label="Search all chats" />
        </label>
      </div>
      <nav className="conv-list">
        {hits !== null ? (
          <>
            <div className="sidebar-label">{hits.length ? `${hits.length} match${hits.length === 1 ? "" : "es"}` : "Search"}</div>
            {hits.length === 0 && <p className="faint" style={{ padding: "4px 14px", fontSize: "0.9em" }}>No chats mention that.</p>}
            {hits.map((h) => (
              <button
                key={h.conversationId}
                className="search-hit"
                aria-current={h.conversationId === currentId}
                onClick={() => void selectChat(h.conversationId)}
              >
                <span className="t">{h.title}</span>
                {h.snippet && (
                  <span className="s">
                    <Snippet text={h.snippet} />
                  </span>
                )}
              </button>
            ))}
          </>
        ) : (
          <>
            {cloudNotice && workspace !== "local" && <p className="banner sidebar-notice">{cloudNotice}</p>}
            {empty && (
              <p className="faint" style={{ padding: "12px 14px", fontSize: "0.9em" }}>
                {workspace === "cloud"
                  ? cloudChats === null
                    ? "Loading your cloud chats…"
                    : "No chats on your cloud yet. Start one with the new-chat button."
                  : workspace === "both"
                    ? "Ask anything: this Mac and your cloud both answer, and you keep the better one."
                    : "Your conversations will appear here."}
              </p>
            )}
            {sections.pinned.length > 0 && (
              <div>
                <div className="sidebar-label">Pinned</div>
                {sections.pinned.map((c) => (
                  <ConvRow key={c.id} c={c} active={c.id === currentId} folders={folderNames} />
                ))}
              </div>
            )}
            {workspace === "local" && !kids && (
            <div className="sidebar-label with-action">
              <span>Projects</span>
              <button
                className="icon-btn"
                title="New project: chats that share instructions"
                onClick={() => setEditingProject({ id: "", name: "", instructions: "", createdAt: 0 })}
              >
                <FolderPlus size={14} />
              </button>
            </div>
            )}
            {sections.projects.map(({ project, items }) => {
              const key = `project:${project.id}`;
              return (
                <div key={project.id}>
                  <div className="folder-head project-head">
                    <button className="grow-btn" onClick={() => toggleFolder(key)} aria-expanded={!closed.has(key)}>
                      <ChevronRight size={13} style={{ transform: closed.has(key) ? undefined : "rotate(90deg)" }} />
                      <Briefcase size={14} />
                      <span>{project.name}</span>
                    </button>
                    <button className="icon-btn" title={`New chat in ${project.name}`} onClick={() => newChat(false, project.id)}>
                      <Plus size={14} />
                    </button>
                    <button className="icon-btn" title="Project settings" onClick={() => setEditingProject(project)}>
                      <MoreHorizontal size={14} />
                    </button>
                  </div>
                  {!closed.has(key) &&
                    (items.length ? (
                      items.map((c) => <ConvRow key={c.id} c={c} active={c.id === currentId} folders={folderNames} indent />)
                    ) : (
                      <p className="faint project-empty">No chats yet</p>
                    ))}
                </div>
              );
            })}
            {sections.folders.map((f) => (
              <div key={f.name}>
                <button className="folder-head" onClick={() => toggleFolder(f.name)} aria-expanded={!closed.has(f.name)}>
                  <ChevronRight size={13} style={{ transform: closed.has(f.name) ? undefined : "rotate(90deg)" }} />
                  <Folder size={14} />
                  <span>{f.name}</span>
                  <span className="count">{f.items.length}</span>
                </button>
                {!closed.has(f.name) &&
                  f.items.map((c) => <ConvRow key={c.id} c={c} active={c.id === currentId} folders={folderNames} indent />)}
              </div>
            ))}
            {sections.dated.map((g) => (
              <div key={g.label}>
                <div className="sidebar-label">{g.label}</div>
                {g.items.map((c) => (
                  <ConvRow key={c.id} c={c} active={c.id === currentId} folders={folderNames} />
                ))}
              </div>
            ))}
            {workspace === "cloud" && (remoteOnly.length > 0 || cloudChats !== null) && (
              <div>
                <div className="sidebar-label with-action">
                  <span>On your cloud</span>
                  <button className="icon-btn" title="Refresh the list from your cloud" onClick={() => void refreshCloudChats()}>
                    <RefreshCw size={13} />
                  </button>
                </div>
                {remoteOnly.map((c) => (
                  <CloudRow key={c.id} c={c} />
                ))}
                {remoteOnly.length === 0 && <p className="faint project-empty">All of them are listed above.</p>}
              </div>
            )}
          </>
        )}
      </nav>
      {editingProject && createPortal(<ProjectEditor project={editingProject} onClose={() => setEditingProject(null)} />, document.body)}
    </aside>
  );
}

/** A cloud conversation not opened on this Mac yet: click to open, or delete it on the cloud. */
function CloudRow({ c }: { c: CloudChat }) {
  const openCloudChat = useStore((s) => s.openCloudChat);
  const deleteCloudChat = useStore((s) => s.deleteCloudChat);
  const [busy, setBusy] = useState(false);
  return (
    <div className="conv-row cloud-row">
      <button
        className="conv-item"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          await openCloudChat(c.id);
          setBusy(false);
        }}
        title="Open this cloud chat"
      >
        <Cloud size={14} className="faint" />
        <span className="t">{c.title}</span>
      </button>
      <button
        className="icon-btn del"
        title="Delete on your cloud"
        aria-label={`Delete ${c.title} on your cloud`}
        onClick={async () => {
          if (await ask(`Delete “${c.title}” on your BYTE cloud? This can't be undone.`, { title: "Delete cloud chat", kind: "warning" }).catch(() => true))
            await deleteCloudChat(c.id);
        }}
      >
        <Trash2 size={14} />
      </button>
    </div>
  );
}

/** Create or edit a project: its name and the instructions every chat in it follows. */
function ProjectEditor({ project, onClose }: { project: Project; onClose(): void }) {
  const saveProject = useStore((s) => s.saveProject);
  const deleteProject = useStore((s) => s.deleteProject);
  const newChat = useStore((s) => s.newChat);
  const [name, setName] = useState(project.name);
  const [instructions, setInstructions] = useState(project.instructions);
  const [error, setError] = useState<string | null>(null);
  const isNew = !project.id;

  const save = async () => {
    try {
      const saved = await saveProject({ ...project, name, instructions });
      if (isNew) newChat(false, saved.id);
      onClose();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal small" role="dialog" aria-modal="true" aria-label={isNew ? "New project" : "Project settings"}>
        <section className="modal-body">
          <button className="icon-btn modal-close" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
          <h3>{isNew ? "New project" : "Project settings"}</h3>
          <p className="muted" style={{ marginTop: 0 }}>Chats in a project share its instructions, like the budget for a remodel or the style for a class.</p>
          {error && <div className="banner danger">{error}</div>}
          <label className="form-field">
            <span>Name</span>
            <input className="text-input" autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Kitchen remodel" />
          </label>
          <label className="form-field">
            <span>Instructions for BYTE</span>
            <textarea
              className="about-me"
              rows={6}
              value={instructions}
              maxLength={4000}
              onChange={(e) => setInstructions(e.target.value)}
              placeholder="Budget is $20,000. The kitchen is 12 × 14 ft. We prefer light wood and want to keep the window."
            />
          </label>
          <div className="row" style={{ gap: 8, marginTop: 12 }}>
            {!isNew && (
              <button
                className="btn sm ghost"
                style={{ color: "var(--danger)" }}
                onClick={async () => {
                  if (await ask(`Delete the project “${project.name}”? Its chats are kept.`, { title: "Delete project", kind: "warning" })) {
                    await deleteProject(project.id);
                    onClose();
                  }
                }}
              >
                <Trash2 size={14} /> Delete project
              </button>
            )}
            <span className="spacer" style={{ flex: 1 }} />
            <button className="btn sm ghost" onClick={onClose}>Cancel</button>
            <button className="btn sm primary" disabled={!name.trim()} onClick={() => void save()}>
              {isNew ? "Create" : "Save"}
            </button>
          </div>
        </section>
      </div>
    </div>
  );
}

function ConvRow({ c, active, folders, indent }: { c: Conversation; active: boolean; folders: string[]; indent?: boolean }) {
  const selectChat = useStore((s) => s.selectChat);
  const deleteChat = useStore((s) => s.deleteChat);
  const updateChat = useStore((s) => s.updateChat);
  const projects = useStore((s) => s.projects);
  const [menu, setMenu] = useState(false);
  const [editing, setEditing] = useState<"title" | "folder" | null>(null);
  const [value, setValue] = useState("");
  const ref = useRef<HTMLDivElement>(null);

  // Close the menu on outside click.
  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setMenu(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menu]);

  const commit = () => {
    if (editing === "title" && value.trim()) void updateChat(c.id, { title: value });
    if (editing === "folder") void updateChat(c.id, { folder: value });
    setEditing(null);
  };

  if (editing) {
    return (
      <div className="conv-row">
        <input
          className="conv-edit"
          autoFocus
          value={value}
          placeholder={editing === "folder" ? "Folder name" : "Chat title"}
          aria-label={editing === "folder" ? "Folder name" : "Chat title"}
          onChange={(e) => setValue(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            if (e.key === "Escape") setEditing(null);
          }}
        />
      </div>
    );
  }

  return (
    <div className={`conv-row ${indent ? "indent" : ""}`} ref={ref}>
      <button
        className="conv-item"
        aria-current={active}
        onClick={() => void selectChat(c.id)}
        title={[c.title, c.summary, c.tags?.length ? c.tags.map((t) => `#${t}`).join(" ") : ""].filter(Boolean).join("\n")}
      >
        {c.private ? <Lock size={14} /> : c.pinned ? <Pin size={14} /> : <MessageSquare size={15} />}
        <span>{c.title}</span>
      </button>
      <button className="icon-btn del" aria-label={`More actions for ${c.title}`} title="More" onClick={() => setMenu(!menu)}>
        <MoreHorizontal size={15} />
      </button>
      {menu && (
        <div className="row-menu" role="menu">
          {!c.private && (
            <>
              <button role="menuitem" onClick={() => { setMenu(false); void updateChat(c.id, { pinned: !c.pinned }); }}>
                {c.pinned ? <PinOff size={14} /> : <Pin size={14} />} {c.pinned ? "Unpin" : "Pin"}
              </button>
              <button role="menuitem" onClick={() => { setMenu(false); setValue(c.title); setEditing("title"); }}>
                <Pencil size={14} /> Rename
              </button>
              {folders
                .filter((f) => f !== c.folder)
                .map((f) => (
                  <button key={f} role="menuitem" onClick={() => { setMenu(false); void updateChat(c.id, { folder: f }); }}>
                    <FolderInput size={14} /> Move to {f}
                  </button>
                ))}
              {projects
                .filter((p) => p.id !== c.projectId && spaceOf(c.id) === "local")
                .map((p) => (
                  <button key={p.id} role="menuitem" onClick={() => { setMenu(false); void updateChat(c.id, { projectId: p.id }); }}>
                    <Briefcase size={14} /> Move to {p.name}
                  </button>
                ))}
              {c.projectId && (
                <button role="menuitem" onClick={() => { setMenu(false); void updateChat(c.id, { projectId: "" }); }}>
                  <Briefcase size={14} /> Remove from project
                </button>
              )}
              <button role="menuitem" onClick={() => { setMenu(false); setValue(""); setEditing("folder"); }}>
                <Folder size={14} /> New folder…
              </button>
              {c.folder && (
                <button role="menuitem" onClick={() => { setMenu(false); void updateChat(c.id, { folder: "" }); }}>
                  <FolderInput size={14} /> Remove from folder
                </button>
              )}
            </>
          )}
          <button
            role="menuitem"
            className="danger"
            onClick={async () => {
              setMenu(false);
              const onCloud = !!c.cloudId && spaceOf(c.id) === "cloud";
              const ok = await ask(
                onCloud ? `Delete “${c.title}” here and on your BYTE cloud? This can't be undone.` : `Delete “${c.title}”? This can't be undone.`,
                { title: "Delete chat", kind: "warning" },
              );
              if (ok) deleteChat(c.id);
            }}
          >
            <Trash2 size={14} /> Delete
          </button>
        </div>
      )}
    </div>
  );
}
