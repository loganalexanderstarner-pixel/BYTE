import { Bot, MessageSquarePlus, Pencil, Plus, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import type { Assistant } from "../../lib/types";
import { useStore } from "../../state/store";

const MODES: [string, string][] = [
  ["", "Current mode"],
  ["fast", "Fast"],
  ["auto", "Auto"],
  ["deep", "Deep"],
  ["extended", "Extended"],
];

function blank(): Assistant {
  return { id: "", name: "", emoji: "🤖", instructions: "", starters: [], mode: "", created: 0 };
}

function Editor({ value, onSave, onCancel }: { value: Assistant; onSave: (a: Assistant) => void; onCancel: () => void }) {
  const [a, setA] = useState(value);
  const starters = [...a.starters, "", "", "", ""].slice(0, 4);
  return (
    <div className="job-editor">
      <div className="job-grid" style={{ gridTemplateColumns: "80px 1fr 160px" }}>
        <label>
          Emoji <input value={a.emoji} onChange={(e) => setA({ ...a, emoji: e.target.value })} maxLength={8} />
        </label>
        <label>
          Name <input value={a.name} onChange={(e) => setA({ ...a, name: e.target.value })} placeholder="e.g. Email helper" />
        </label>
        <label>
          Mode
          <select value={a.mode} onChange={(e) => setA({ ...a, mode: e.target.value })}>
            {MODES.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label>
        Instructions
        <textarea
          rows={7}
          value={a.instructions}
          onChange={(e) => setA({ ...a, instructions: e.target.value })}
          placeholder="What it's for and how it should answer: its job, your level, the style you like, what to always or never do…"
        />
      </label>
      <label>
        Starter prompts (shown on its empty chat)
        {starters.map((s, i) => (
          <input
            key={i}
            value={s}
            onChange={(e) => {
              const next = [...starters];
              next[i] = e.target.value;
              setA({ ...a, starters: next });
            }}
            placeholder={i === 0 ? "e.g. Reply to this email politely saying no:" : ""}
          />
        ))}
      </label>
      <div className="row" style={{ gap: 8, justifyContent: "flex-end" }}>
        <button className="btn sm" onClick={onCancel}>
          Cancel
        </button>
        <button className="btn sm primary" disabled={!a.name.trim()} onClick={() => onSave({ ...a, starters: starters.filter((s) => s.trim()) })}>
          Save
        </button>
      </div>
    </div>
  );
}

/** Custom assistants (🤖): BYTE set up for one job, with its own instructions and starters. */
export function AssistantsPanel({ onClose }: { onClose: () => void }) {
  const newChat = useStore((s) => s.newChat);
  const [list, setList] = useState<Assistant[] | null>(null);
  const [presets, setPresets] = useState<Assistant[]>([]);
  const [editing, setEditing] = useState<Assistant | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    api.assistantsList().then(setList, (e) => setError(errorText(e)));
  }, []);
  useEffect(() => {
    load();
    api.assistantPresets().then(setPresets, () => {});
  }, [load]);
  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const start = (a: Assistant) => {
    newChat(false, null, { id: a.id, mode: a.mode });
    onClose();
  };
  const unused = presets.filter((p) => !(list ?? []).some((a) => a.name === p.name));

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="study-panel" role="dialog" aria-modal="true" aria-label="Assistants">
        <div className="recipe-box-head">
          <Bot size={18} />
          <h2>{editing ? (editing.id ? `Edit ${editing.name}` : "New assistant") : "Assistants"}</h2>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        {error && <div className="banner error">{error}</div>}
        {editing ? (
          <Editor
            value={editing}
            onCancel={() => setEditing(null)}
            onSave={(a) =>
              void guard(async () => {
                await api.assistantSave(a);
                setEditing(null);
                load();
              })
            }
          />
        ) : (
          <>
            <p className="muted small" style={{ marginTop: 12 }}>
              An assistant is BYTE set up for one job: its instructions apply to every answer in its chats. Start a chat with one to use it.
            </p>
            {list && list.length > 0 && (
              <ul className="decks">
                {list.map((a) => (
                  <li key={a.id}>
                    <span className="assistant-emoji">{a.emoji}</span>
                    <div className="deck-info">
                      <b>{a.name}</b>
                      <span className="muted small clamp-1">{a.instructions || "No instructions yet"}</span>
                    </div>
                    <button className="btn sm primary" onClick={() => start(a)}>
                      <MessageSquarePlus size={14} /> Chat
                    </button>
                    <button className="icon-btn" title="Edit" aria-label={`Edit ${a.name}`} onClick={() => setEditing(a)}>
                      <Pencil size={15} />
                    </button>
                    <button className="icon-btn" title="Delete" aria-label={`Delete ${a.name}`} onClick={() => void guard(async () => (await api.assistantDelete(a.id), load()))}>
                      <Trash2 size={15} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
            <div className="row" style={{ gap: 8, marginTop: 12, flexWrap: "wrap" }}>
              <button className="btn sm" onClick={() => setEditing(blank())}>
                <Plus size={14} /> New assistant
              </button>
              {unused.map((p) => (
                <button key={p.id} className="btn sm" title={p.instructions} onClick={() => setEditing({ ...p, id: "" })}>
                  {p.emoji} {p.name}
                </button>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
