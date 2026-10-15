import { Brain, Group as GroupIcon, Loader2, NotebookPen, Plus, Shapes, Sparkles, Trash2, Wand2, X } from "lucide-react";
import { useCallback, useEffect, useReducer, useRef, useState, type PointerEvent } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { boardOutline, boardReducer, COLORS, STICKY_H, STICKY_W, type BoardData } from "../../lib/board";
import type { BoardInfo } from "../../lib/types";
import { useStore } from "../../state/store";

const EMPTY: BoardData = { stickies: [], groups: [] };

/** The brainstorm board: sticky notes BYTE can add to, expand and group into themes. */
export function BoardPanel() {
  const req = useStore((s) => s.board);
  const close = useStore((s) => s.closeBoard);
  const openNotes = useStore((s) => s.openNotes);
  const openMindmap = useStore((s) => s.openMindmap);
  const [boards, setBoards] = useState<BoardInfo[]>([]);
  const [id, setId] = useState(0);
  const [title, setTitle] = useState(req?.topic ?? "");
  const [data, act] = useReducer(boardReducer, EMPTY);
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const drag = useRef<{ id: string; dx: number; dy: number } | null>(null);
  const loaded = useRef(false);

  const refresh = useCallback(() => {
    if (inTauri) void api.boardsList().then(setBoards, () => undefined);
  }, []);
  useEffect(refresh, [refresh]);

  // Save a moment after each change.
  useEffect(() => {
    if (!inTauri || !loaded.current || (!data.stickies.length && !id)) return;
    const t = setTimeout(() => {
      void api.boardSave(id, title, data).then((newId) => {
        if (newId !== id) setId(newId);
        refresh();
      }, (e) => setError(errorText(e)));
    }, 600);
    return () => clearTimeout(t);
  }, [data, title, id, refresh]);

  const assist = async (kind: "ideas" | "expand" | "group") => {
    if (!title.trim()) return setError("Give the board a topic first (the title).");
    setError(null);
    setBusy(kind);
    try {
      const texts = data.stickies.map((s) => s.text);
      const focus = kind === "expand" ? data.stickies.find((s) => s.id === selected)?.text : undefined;
      const r = await api.boardAssist(kind, title, texts, focus);
      if (kind === "group") act({ type: "group", groups: r.groups });
      else if (r.ideas.length) act({ type: "addMany", texts: r.ideas, near: kind === "expand" ? (selected ?? undefined) : undefined });
      else setError("BYTE didn't come up with anything new; try again.");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  // The first time: a new board on a topic gets ideas right away.
  useEffect(() => {
    loaded.current = true;
    if (req?.topic && inTauri) void assist("ideas");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const open = async (b: BoardInfo) => {
    const full = await api.boardGet(b.id);
    setId(full.id);
    setTitle(full.title);
    act({ type: "load", data: full.data as BoardData });
    setSelected(null);
  };
  const fresh = () => {
    setId(0);
    setTitle("");
    act({ type: "load", data: EMPTY });
  };

  const down = (e: PointerEvent, sid: string) => {
    if (editing === sid) return;
    const s = data.stickies.find((x) => x.id === sid)!;
    drag.current = { id: sid, dx: e.clientX - s.x, dy: e.clientY - s.y };
    setSelected(sid);
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };
  const move = (e: PointerEvent) => {
    const d = drag.current;
    if (d) act({ type: "move", id: d.id, x: Math.max(0, e.clientX - d.dx), y: Math.max(0, e.clientY - d.dy) });
  };

  const outline = () => boardOutline(title || "Brainstorm", data);

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="writing-panel board-panel" role="dialog" aria-modal="true" aria-label="Brainstorm board">
        <div className="recipe-box-head">
          <Shapes size={18} />
          <input className="note-title grow" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="What are you brainstorming?" aria-label="Board topic" />
          <select value={id || ""} onChange={(e) => (e.target.value ? void open(boards.find((b) => b.id === Number(e.target.value))!) : fresh())} aria-label="Boards">
            <option value="">New board</option>
            {boards.map((b) => (
              <option key={b.id} value={b.id}>{b.title} ({b.count})</option>
            ))}
          </select>
          <button className="icon-btn" onClick={close} aria-label="Close"><X size={16} /></button>
        </div>
        <div className="row board-tools" style={{ gap: 6, flexWrap: "wrap" }}>
          <button className="btn sm ghost" onClick={() => act({ type: "add", text: "New idea" })}><Plus size={13} /> Sticky</button>
          <button className="btn sm primary" disabled={!!busy} onClick={() => void assist("ideas")}>
            {busy === "ideas" ? <Loader2 size={13} className="spin" /> : <Sparkles size={13} />} Add ideas
          </button>
          <button className="btn sm ghost" disabled={!!busy || !selected} onClick={() => void assist("expand")} title="Select a sticky first">
            {busy === "expand" ? <Loader2 size={13} className="spin" /> : <Wand2 size={13} />} Expand this
          </button>
          <button className="btn sm ghost" disabled={!!busy || data.stickies.length < 3} onClick={() => void assist("group")}>
            {busy === "group" ? <Loader2 size={13} className="spin" /> : <GroupIcon size={13} />} Group into themes
          </button>
          <span className="grow" />
          <button className="btn sm ghost" disabled={!data.stickies.length} onClick={() => openMindmap(outline(), title || "Brainstorm")}><Brain size={13} /> Mind map</button>
          <button
            className="btn sm ghost"
            disabled={!data.stickies.length}
            onClick={() => {
              close();
              openNotes({ draft: { title: title || "Brainstorm", folder: "Inbox", tags: ["brainstorm"], body: outline() } });
            }}
          >
            <NotebookPen size={13} /> Save as note
          </button>
          {id > 0 && (
            <button className="icon-btn sm" aria-label="Delete board" onClick={() => void api.boardDelete(id).then(() => (fresh(), refresh()))}>
              <Trash2 size={13} />
            </button>
          )}
        </div>
        {error && <div className="banner danger">{error}</div>}
        <div className="board-canvas" onPointerMove={move} onPointerUp={() => (drag.current = null)} onMouseDown={(e) => e.target === e.currentTarget && setSelected(null)}>
          {data.groups.map((g) => (
            <div key={g.id} className="board-group" style={{ left: g.x, top: g.y, width: g.w, height: g.h }}>
              <b>{g.label}</b>
            </div>
          ))}
          {data.stickies.map((s) => (
            <div
              key={s.id}
              className={`sticky sticky-${s.color} ${selected === s.id ? "on" : ""}`}
              style={{ left: s.x, top: s.y, width: STICKY_W, height: STICKY_H }}
              onPointerDown={(e) => down(e, s.id)}
              onDoubleClick={() => setEditing(s.id)}
            >
              {editing === s.id ? (
                <textarea autoFocus defaultValue={s.text} onBlur={(e) => (act({ type: "edit", id: s.id, text: e.target.value }), setEditing(null))} aria-label="Sticky text" />
              ) : (
                <span>{s.text}</span>
              )}
              {selected === s.id && editing !== s.id && (
                <span className="sticky-tools" onPointerDown={(e) => e.stopPropagation()}>
                  {COLORS.map((c) => (
                    <button key={c} className={`dot sticky-${c}`} aria-label={c} onClick={() => act({ type: "color", id: s.id, color: c })} />
                  ))}
                  <button className="icon-btn sm" aria-label="Delete sticky" onClick={() => act({ type: "delete", id: s.id })}><Trash2 size={11} /></button>
                </span>
              )}
            </div>
          ))}
          {!data.stickies.length && (
            <p className="faint board-empty">
              Type a topic above and press <b>Add ideas</b>, or add stickies yourself. Drag them around, double-click to edit, and let BYTE group them into themes.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
