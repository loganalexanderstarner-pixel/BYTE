import { BookMarked, CalendarDays, Eye, ListTodo, Receipt, Search, Workflow, X } from "lucide-react";
import { useEffect, useState } from "react";

import { api, inTauri } from "../../lib/api";
import { tiles, type Tile } from "../../lib/dashboard";
import type { DashboardSummary, Researched } from "../../lib/types";
import { useStore } from "../../state/store";

const ICONS: Record<string, typeof ListTodo> = { today: CalendarDays, todos: ListTodo, coming: Receipt, watch: Eye, auto: Workflow, research: BookMarked };

/** Chats with cited answers, searchable (Rust `dashboard::research`). */
function ResearchLibrary({ onClose }: { onClose: () => void }) {
  const [q, setQ] = useState("");
  const [list, setList] = useState<Researched[] | null>(null);
  const selectChat = useStore((s) => s.selectChat);
  useEffect(() => {
    const t = setTimeout(() => api.researchLibrary(q).then(setList, () => setList([])), 150);
    return () => clearTimeout(t);
  }, [q]);
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="study-panel" role="dialog" aria-modal="true" aria-label="Research library">
        <div className="recipe-box-head">
          <BookMarked size={18} />
          <h2>Research library</h2>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        <div className="auto-row">
          <Search size={14} />
          <input className="grow" autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search your researched chats" aria-label="Search research" />
        </div>
        {list == null ? (
          <p className="muted">Loading…</p>
        ) : list.length === 0 ? (
          <p className="muted">{q ? "No researched chats match that." : "Answers with sources (web research, papers, your files) collect here."}</p>
        ) : (
          <ul className="schedule-list">
            {list.map((r) => (
              <li key={r.id}>
                <BookMarked size={15} />
                <button
                  className="grow link-row"
                  onClick={() => {
                    void selectChat(r.id);
                    onClose();
                  }}
                >
                  <b>{r.title}</b>
                  <span className="muted small">
                    {new Date(r.updatedAt).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })} · {r.sources} source{r.sources === 1 ? "" : "s"}
                    {r.examples.length > 0 && ` · ${r.examples.join(", ")}`}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

/** The command-deck home: live tiles from BYTE's own data (nothing when there's nothing to show). */
export function Deck() {
  const [summary, setSummary] = useState<DashboardSummary | null>(null);
  const [today, setToday] = useState<[string, string][] | null>(null);
  const [library, setLibrary] = useState(false);
  const send = useStore((s) => s.send);
  const ready = useStore((s) => s.engine.state === "ready");
  useEffect(() => {
    if (!inTauri && !(window as { __emit?: unknown }).__emit) return;
    api.dashboardSummary().then(setSummary, () => setSummary(null));
    api.dashboardToday().then(setToday, () => setToday(null));
  }, []);
  const shown = tiles(summary, today, Date.now());
  if (shown.length === 0) return library ? <ResearchLibrary onClose={() => setLibrary(false)} /> : null;
  const open = (t: Tile) => {
    if (t.id === "research") return setLibrary(true);
    if (t.ask) void send(t.ask);
  };
  return (
    <>
      <div className="deck" aria-label="Today at a glance">
        {shown.map((t) => {
          const Icon = ICONS[t.id] ?? ListTodo;
          const clickable = t.id === "research" || (!!t.ask && ready);
          return (
            <button key={t.id} className={`deck-tile ${t.tone ?? ""}`} disabled={!clickable} onClick={() => open(t)} title={t.ask ?? undefined}>
              <b>
                <Icon size={15} /> {t.title}
              </b>
              {t.lines.map((l, i) => (
                <span key={i} className="small">
                  {l}
                </span>
              ))}
            </button>
          );
        })}
      </div>
      {library && <ResearchLibrary onClose={() => setLibrary(false)} />}
    </>
  );
}
