import { Check, ClipboardList, Copy, Search, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { clipAge, clipPreview } from "../../lib/clips";
import type { Clip } from "../../lib/types";
import { osText } from "../../lib/platform";

/** Clipboard history (Mac control): what you copied lately, searchable; copy an item back with one click. */
export function ClipboardPanel({ onClose }: { onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [clips, setClips] = useState<Clip[] | null>(null);
  const [copied, setCopied] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    api.clipList(query).then(setClips, (e) => setError(errorText(e)));
  }, [query]);
  useEffect(() => {
    const t = setTimeout(load, 150);
    return () => clearTimeout(t);
  }, [load]);
  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const copy = (c: Clip) =>
    void guard(async () => {
      await api.clipCopy(c.id);
      setCopied(c.id);
      setTimeout(() => setCopied(null), 1500);
      load();
    });

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="study-panel clips-panel" role="dialog" aria-modal="true" aria-label="Clipboard history">
        <div className="recipe-box-head">
          <ClipboardList size={18} />
          <h2>Clipboard history</h2>
          <span className="spacer" />
          {clips && clips.length > 0 && (
            <button className="btn sm ghost" onClick={() => void guard(async () => (await api.clipClear(), load()))}>
              <Trash2 size={14} /> Clear all
            </button>
          )}
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        <div className="row clips-search">
          <Search size={15} className="faint" />
          <input className="grow" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search what you copied" aria-label="Search clipboard history" autoFocus />
        </div>
        {error && <div className="banner danger">{error}</div>}
        {clips == null ? (
          <p className="muted">Loading…</p>
        ) : clips.length === 0 ? (
          <div className="empty-box">
            <p>{query ? "Nothing matches." : "Nothing copied yet."}</p>
            <p className="muted">Text you copy shows up here (the last 200). Passwords from password managers are never kept.</p>
          </div>
        ) : (
          <ul className="clips">
            {clips.map((c) => (
              <li key={c.id}>
                <button className="clip-text link-like" onClick={() => copy(c)} title="Copy this again">
                  <span className="clamp-2">{clipPreview(c.text)}</span>
                  <span className="faint small">{clipAge(c.at, Date.now())}</span>
                </button>
                <button className="icon-btn" onClick={() => copy(c)} aria-label="Copy" title="Copy">
                  {copied === c.id ? <Check size={15} /> : <Copy size={15} />}
                </button>
                <button className="icon-btn" onClick={() => void guard(async () => (await api.clipDelete(c.id), load()))} aria-label="Delete" title="Delete">
                  <Trash2 size={15} />
                </button>
              </li>
            ))}
          </ul>
        )}
        <p className="faint small" style={{ marginTop: 12 }}>
          {osText("Kept encrypted on this Mac only. Turn it off in Settings → About → Features → Clipboard history.")}
        </p>
      </div>
    </div>
  );
}
