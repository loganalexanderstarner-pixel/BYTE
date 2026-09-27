import { ask } from "@tauri-apps/plugin-dialog";
import { MessageSquare, PanelLeft, Search, SquarePen, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";

import { Logo } from "../design/Logo";
import { useStore } from "../state/store";

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

export function Sidebar() {
  const conversations = useStore((s) => s.conversations);
  const currentId = useStore((s) => s.currentId);
  const selectChat = useStore((s) => s.selectChat);
  const deleteChat = useStore((s) => s.deleteChat);
  const newChat = useStore((s) => s.newChat);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const [query, setQuery] = useState("");

  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = conversations.filter(
      (c) =>
        c.messages.length > 0 &&
        (!q || c.title.toLowerCase().includes(q) || c.messages.some((m) => m.content.toLowerCase().includes(q))),
    );
    const out: { label: string; items: typeof list }[] = [];
    for (const c of list) {
      const label = groupLabel(c.updatedAt);
      const g = out.find((x) => x.label === label);
      if (g) g.items.push(c);
      else out.push({ label, items: [c] });
    }
    return out;
  }, [conversations, query]);

  return (
    <aside className="sidebar" aria-label="Conversations">
      <div className="titlebar" data-tauri-drag-region>
        <button className="icon-btn" onClick={toggleSidebar} title="Hide sidebar (⌘\)">
          <PanelLeft size={18} />
        </button>
        <button className="icon-btn" onClick={newChat} title="New chat (⌘N)">
          <SquarePen size={18} />
        </button>
      </div>
      <div className="sidebar-brand">
        <Logo size={26} />
        <span className="wordmark">BYTE</span>
      </div>
      <div className="sidebar-section">
        <label className="row" style={{ gap: 8, padding: "0 10px", height: 32, borderRadius: 10, background: "var(--surface-2)", border: "1px solid var(--border)" }}>
          <Search size={15} className="faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search chats"
            aria-label="Search chats"
            style={{ flex: 1, border: 0, outline: "none", background: "transparent", minWidth: 0 }}
          />
        </label>
      </div>
      <nav className="conv-list">
        {groups.length === 0 && (
          <p className="faint" style={{ padding: "12px 14px", fontSize: "0.9em" }}>
            {query ? "No chats match." : "Your conversations will appear here."}
          </p>
        )}
        {groups.map((g) => (
          <div key={g.label}>
            <div className="sidebar-label">{g.label}</div>
            {g.items.map((c) => (
              <div key={c.id} className="conv-row">
                <button
                  className="conv-item"
                  aria-current={c.id === currentId}
                  onClick={() => selectChat(c.id)}
                  title={c.title}
                >
                  <MessageSquare size={15} />
                  <span>{c.title}</span>
                </button>
                <button
                  className="icon-btn del"
                  aria-label={`Delete ${c.title}`}
                  title="Delete chat"
                  onClick={async () => {
                    const ok = await ask(`Delete “${c.title}”? This can't be undone.`, { title: "Delete chat", kind: "warning" });
                    if (ok) deleteChat(c.id);
                  }}
                >
                  <Trash2 size={14} />
                </button>
              </div>
            ))}
          </div>
        ))}
      </nav>
    </aside>
  );
}
