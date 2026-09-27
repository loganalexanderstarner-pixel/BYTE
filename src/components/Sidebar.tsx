import { ask } from "@tauri-apps/plugin-dialog";
import {
  ChevronRight,
  EyeOff,
  Folder,
  FolderInput,
  Lock,
  MessageSquare,
  MoreHorizontal,
  PanelLeft,
  Pencil,
  Pin,
  PinOff,
  Search,
  SquarePen,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Logo } from "../design/Logo";
import { api, inTauri } from "../lib/api";
import type { SearchHit } from "../lib/types";
import { hasMessages, useStore, type Conversation } from "../state/store";

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

/** Sidebar sections: pinned, folders, then the rest by date. */
export function sidebarSections(conversations: Conversation[]) {
  const visible = conversations.filter(hasMessages);
  const pinned = visible.filter((c) => c.pinned);
  const folders = new Map<string, Conversation[]>();
  const dated: { label: string; items: Conversation[] }[] = [];
  for (const c of visible) {
    if (c.pinned) continue;
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
    folders: [...folders.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([name, items]) => ({ name, items: items.sort(byRecent) })),
    dated,
  };
}

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
  const conversations = useStore((s) => s.conversations);
  const currentId = useStore((s) => s.currentId);
  const selectChat = useStore((s) => s.selectChat);
  const newChat = useStore((s) => s.newChat);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
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

  const sections = useMemo(() => sidebarSections(conversations), [conversations]);
  const folderNames = sections.folders.map((f) => f.name);
  const empty = !sections.pinned.length && !sections.folders.length && !sections.dated.length;

  const toggleFolder = (name: string) => {
    const next = new Set(closed);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    setClosed(next);
  };

  return (
    <aside className="sidebar" aria-label="Conversations">
      <div className="titlebar" data-tauri-drag-region>
        <button className="icon-btn" onClick={toggleSidebar} title="Hide sidebar (⌘\)">
          <PanelLeft size={18} />
        </button>
        <button className="icon-btn" onClick={() => newChat(true)} title="New private chat: not saved, doesn't use memory">
          <EyeOff size={17} />
        </button>
        <button className="icon-btn" onClick={() => newChat()} title="New chat (⌘N)">
          <SquarePen size={18} />
        </button>
      </div>
      <div className="sidebar-brand">
        <Logo size={26} />
        <span className="wordmark">BYTE</span>
      </div>
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
            {empty && <p className="faint" style={{ padding: "12px 14px", fontSize: "0.9em" }}>Your conversations will appear here.</p>}
            {sections.pinned.length > 0 && (
              <div>
                <div className="sidebar-label">Pinned</div>
                {sections.pinned.map((c) => (
                  <ConvRow key={c.id} c={c} active={c.id === currentId} folders={folderNames} />
                ))}
              </div>
            )}
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
          </>
        )}
      </nav>
    </aside>
  );
}

function ConvRow({ c, active, folders, indent }: { c: Conversation; active: boolean; folders: string[]; indent?: boolean }) {
  const selectChat = useStore((s) => s.selectChat);
  const deleteChat = useStore((s) => s.deleteChat);
  const updateChat = useStore((s) => s.updateChat);
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
      <button className="conv-item" aria-current={active} onClick={() => void selectChat(c.id)} title={c.title}>
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
              const ok = await ask(`Delete “${c.title}”? This can't be undone.`, { title: "Delete chat", kind: "warning" });
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
