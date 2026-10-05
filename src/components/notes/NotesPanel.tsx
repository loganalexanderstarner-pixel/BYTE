import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Brain, Eye, FolderOpen, MessageSquare, NotebookPen, Pencil, Plus, Save, Search, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { renderMarkdown } from "../../lib/markdown";
import { allTags, filterNotes, noteAge, parseTags } from "../../lib/notes";
import type { Note, NoteInput, NotesInfo } from "../../lib/types";
import { useStore } from "../../state/store";
import { osText } from "../../lib/platform";

const EMPTY: NoteInput = { title: "", folder: "Inbox", tags: [], body: "" };

/** 📝 Notes: your Markdown notes (files in Documents/BYTE/Notes), with folders, tags and search. */
export function NotesPanel() {
  const start = useStore((s) => s.notes);
  const close = useStore((s) => s.closeNotes);
  const openMindmap = useStore((s) => s.openMindmap);
  const send = useStore((s) => s.send);
  const newChat = useStore((s) => s.newChat);
  const [notes, setNotes] = useState<Note[]>([]);
  const [info, setInfo] = useState<NotesInfo | null>(null);
  const [query, setQuery] = useState("");
  const [folder, setFolder] = useState<string>("");
  const [tag, setTag] = useState<string>("");
  const [edit, setEdit] = useState<NoteInput | null>(start?.draft ? { ...EMPTY, ...start.draft } : null);
  const [tagsText, setTagsText] = useState((start?.draft?.tags ?? []).join(", "));
  const [preview, setPreview] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const refresh = useCallback(async () => {
    if (!inTauri) return;
    try {
      const [list, i] = await Promise.all([api.notesList(), api.notesInfo()]);
      setNotes(list);
      setInfo(i);
      return list;
    } catch (e) {
      setError(errorText(e));
    }
  }, []);
  useEffect(() => {
    void refresh().then((list) => {
      const pick = start?.id && list?.find((n) => n.id === start.id);
      if (pick) open(pick);
    });
    const off = inTauri ? api.onNoteClipped(() => void refresh()) : null;
    return () => void off?.then((f) => f());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refresh]);

  const mindmapOpen = useStore((s) => !!s.mindmap);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !mindmapOpen && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close, mindmapOpen]);

  const open = (n: Note) => {
    setEdit({ id: n.id, title: n.title, folder: n.folder, tags: n.tags, body: n.body, source: n.source, chat: n.chat });
    setTagsText(n.tags.join(", "));
    setPreview(true);
    setSaved(false);
  };
  const current = edit?.id ? notes.find((n) => n.id === edit.id) : undefined;
  const shown = useMemo(() => filterNotes(notes, { query, folder, tag }), [notes, query, folder, tag]);
  const tags = useMemo(() => allTags(notes), [notes]);

  const save = async () => {
    if (!edit) return;
    setError(null);
    try {
      const n = await api.noteSave({ ...edit, title: edit.title.trim() || "Untitled", tags: parseTags(tagsText) });
      setEdit({ ...edit, id: n.id, title: n.title, folder: n.folder, tags: n.tags });
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
      await refresh();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const remove = async () => {
    if (!edit?.id) return setEdit(null);
    try {
      await api.noteDelete(edit.id);
      setEdit(null);
      await refresh();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const ask = () => {
    if (!edit) return;
    close();
    newChat();
    void send(`About my note "${edit.title}":\n\n${edit.body.slice(0, 12000)}\n\n`);
  };

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="writing-panel notes-panel" role="dialog" aria-modal="true" aria-label="Notes">
        <div className="recipe-box-head">
          <NotebookPen size={18} />
          <h2>Notes</h2>
          {info && (
            <button className="linklike faint small" onClick={() => void revealItemInDir(info.dir).catch(() => undefined)} title="Your notes are plain Markdown files here">
              <FolderOpen size={12} /> {info.dir.replace(/^\/Users\/[^/]+/, "~")}
            </button>
          )}
          <span className="grow" />
          <button className="btn sm primary" onClick={() => (setEdit({ ...EMPTY, folder: folder || "Inbox" }), setTagsText(""), setPreview(false))}>
            <Plus size={13} /> New note
          </button>
          <button className="icon-btn" onClick={close} aria-label="Close">
            <X size={16} />
          </button>
        </div>
        {error && <div className="banner danger">{error}</div>}
        <div className="notes-body">
          <div className="notes-list">
            <label className="search">
              <Search size={14} className="faint" />
              <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search notes" aria-label="Search notes" />
            </label>
            <div className="chips" role="group" aria-label="Folder">
              <button className="chip" aria-pressed={!folder} onClick={() => setFolder("")}>All ({notes.length})</button>
              {(info?.folders ?? []).map((f) => (
                <button key={f} className="chip" aria-pressed={folder === f} onClick={() => setFolder(folder === f ? "" : f)}>{f}</button>
              ))}
            </div>
            {tags.length > 0 && (
              <div className="chips" role="group" aria-label="Tag">
                {tags.slice(0, 12).map(([t, n]) => (
                  <button key={t} className="chip" aria-pressed={tag === t} onClick={() => setTag(tag === t ? "" : t)}>#{t} <span className="faint">{n}</span></button>
                ))}
              </div>
            )}
            {shown.length === 0 && (
              <p className="faint small" style={{ padding: 8 }}>
                {notes.length ? "No notes match." : "No notes yet. Write one, press 📝 under any answer, or clip a web page with the bookmarklet (Settings → Features → Notes)."}
              </p>
            )}
            {shown.map((n) => (
              <button key={n.id} className={`note-row ${edit?.id === n.id ? "on" : ""}`} onClick={() => open(n)}>
                <b>{n.title}</b>
                <span className="faint small">
                  {n.folder} · {noteAge(n.updated)}
                  {n.tags.length ? ` · #${n.tags.slice(0, 3).join(" #")}` : ""}
                </span>
                <span className="faint small note-snippet">{n.body.replace(/\[([^\]]*)\]\([^)]*\)/g, "$1").replace(/[#>*_`[\]]/g, "").slice(0, 110)}</span>
              </button>
            ))}
          </div>
          <div className="notes-editor">
            {edit ? (
              <>
                <input className="note-title" value={edit.title} onChange={(e) => setEdit({ ...edit, title: e.target.value })} placeholder="Title" aria-label="Title" />
                <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                  <select value={edit.folder} onChange={(e) => setEdit({ ...edit, folder: e.target.value })} aria-label="Folder">
                    {[...new Set(["Inbox", ...(info?.folders ?? []), edit.folder])].map((f) => (
                      <option key={f} value={f}>{f}</option>
                    ))}
                  </select>
                  <button
                    className="btn sm ghost"
                    onClick={() => {
                      const f = window.prompt("New folder name");
                      if (f?.trim()) setEdit({ ...edit, folder: f.trim() });
                    }}
                  >
                    <Plus size={12} /> Folder
                  </button>
                  <input className="grow note-tags" value={tagsText} onChange={(e) => setTagsText(e.target.value)} placeholder="Tags, separated by commas" aria-label="Tags" />
                </div>
                {current?.source && (
                  <a className="faint small" href={current.source} target="_blank" rel="noreferrer">From {new URL(current.source).hostname}</a>
                )}
                {preview ? (
                  <div className="note-preview markdown" dangerouslySetInnerHTML={{ __html: renderMarkdown(edit.body) }} />
                ) : (
                  <textarea className="note-text" value={edit.body} onChange={(e) => setEdit({ ...edit, body: e.target.value })} placeholder="Write in Markdown…" aria-label="Note" autoFocus={!edit.id} />
                )}
                <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
                  <button className="btn sm primary" onClick={() => void save()}>
                    <Save size={13} /> {saved ? "Saved" : "Save"}
                  </button>
                  <button className="btn sm ghost" onClick={() => setPreview(!preview)}>
                    {preview ? <><Pencil size={13} /> Edit</> : <><Eye size={13} /> Preview</>}
                  </button>
                  <button className="btn sm ghost" onClick={ask} disabled={!edit.body.trim()} title="Start a chat about this note">
                    <MessageSquare size={13} /> Ask BYTE
                  </button>
                  <button className="btn sm ghost" onClick={() => openMindmap(edit.body, edit.title)} disabled={!edit.body.trim()}>
                    <Brain size={13} /> Mind map
                  </button>
                  <span className="grow" />
                  {current && (
                    <button className="btn sm ghost" onClick={() => void revealItemInDir(current.path).catch(() => undefined)}>
                      <FolderOpen size={13} /> {osText("Show in Finder")}
                    </button>
                  )}
                  <button className="icon-btn sm" onClick={() => void remove()} aria-label="Delete note" title={edit.id ? "Move this note to the Trash" : "Discard"}>
                    <Trash2 size={13} />
                  </button>
                </div>
              </>
            ) : (
              <div className="faint" style={{ margin: "auto", textAlign: "center", padding: 24 }}>
                <NotebookPen size={28} />
                <p>Pick a note, or start a new one.</p>
                <p className="small">Notes are plain Markdown files you own: they open in any editor, and BYTE can answer from them when the knowledge base is on.</p>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
