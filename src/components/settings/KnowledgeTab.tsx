import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderPlus, Loader2, RefreshCw, Sparkles, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { bytes } from "../../lib/format";
import type { KbSource } from "../../lib/types";
import { useStore } from "../../state/store";
import { osText } from "../../lib/platform";

/** "3 min ago", "yesterday"… for when a folder was last read. */
export function ago(ms: number | null, now = Date.now()): string {
  if (!ms) return "not yet";
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return s < 172800 ? "yesterday" : `${Math.round(s / 86400)} days ago`;
}

/** Folder path shown with the home folder as "~". */
export function shortPath(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

export function KnowledgeTab() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const kb = useStore((s) => s.kb);
  const progress = useStore((s) => s.kbProgress);
  const refreshKb = useStore((s) => s.refreshKb);
  const downloads = useStore((s) => s.downloads);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refreshKb();
  }, [refreshKb]);

  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
      await refreshKb();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const addFolder = () =>
    guard(async () => {
      const dir = await openDialog({ directory: true, title: "Choose a folder for BYTE to read" });
      if (typeof dir === "string") await api.kbAdd(dir);
    });

  if (!settings) return null;
  const sources = kb?.sources ?? [];
  const embedDl = kb?.embedKey ? downloads[kb.embedKey] : undefined;
  const embedBusy = embedDl && ["downloading", "resuming", "verifying"].includes(embedDl.phase);
  const total = sources.reduce((a, s) => ({ files: a.files + s.files, chunks: a.chunks + s.chunks, bytes: a.bytes + s.bytes }), { files: 0, chunks: 0, bytes: 0 });

  return (
    <>
      <h3>Knowledge base</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        {osText("Add folders and BYTE can answer from your own documents, notes and PDFs, citing the file and page. Everything is read and stored on this Mac, encrypted.")}
      </p>
      {error && <div className="banner danger">{error}</div>}

      <div className="section">
        <label className="row" style={{ gap: 10, cursor: "pointer" }}>
          <input type="checkbox" checked={settings.kbEnabled} onChange={(e) => void update({ kbEnabled: e.target.checked })} />
          <span>
            <b>Use my files in answers</b>
            <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
              BYTE searches these folders when a question may be answered by them (also the "My files" switch in the chat box). Off:
              folders aren't read or searched.
            </span>
          </span>
        </label>
      </div>

      <div className="section">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h4 style={{ margin: 0 }}>Folders ({sources.length})</h4>
          <div className="row" style={{ gap: 8 }}>
            {sources.length > 0 && (
              <button className="btn sm" onClick={() => void guard(() => api.kbReindex())} disabled={!!progress} title="Read files that changed">
                <RefreshCw size={14} /> Check for changes
              </button>
            )}
            <button className="btn sm primary" onClick={() => void addFolder()}>
              <FolderPlus size={14} /> Add folder
            </button>
          </div>
        </div>
        {progress && (
          <div className="kb-progress" role="status">
            <Loader2 size={13} className="spin" />
            {progress.phase === "reading" ? `Reading ${progress.done + 1} of ${progress.total}: ${progress.file}` : `Preparing search by meaning: ${progress.done} of ${progress.total} passages`}
            <div className="bar" style={{ ["--p" as string]: `${progress.total ? Math.round((progress.done / progress.total) * 100) : 0}%` }} />
          </div>
        )}
        <div className="kb-list">
          {sources.length === 0 && <p className="faint" style={{ margin: 0 }}>No folders yet. Try your Documents folder, or a folder of notes or PDFs.</p>}
          {sources.map((s) => (
            <SourceRow key={s.id} s={s} busy={!!progress} onReindex={() => void guard(() => api.kbReindex(s.id))} onRemove={() => void guard(() => api.kbRemove(s.id))} />
          ))}
        </div>
        {sources.length > 0 && (
          <p className="faint" style={{ fontSize: "0.85em", marginBottom: 0 }}>
            {total.files} files · {total.chunks} passages · {bytes(total.bytes)} of text. Folders are checked for changes when BYTE starts and every 15 minutes.
          </p>
        )}
      </div>

      <div className="section">
        <h4>Search by meaning</h4>
        {kb?.embedInstalled ? (
          <p className="muted" style={{ margin: 0 }}>
            <Sparkles size={13} /> On. BYTE finds passages that mean the same as your question even when the words differ. It uses a small
            model that runs only while searching{kb.embedRunning ? " (running now)" : ""}.
          </p>
        ) : (
          <div className="row" style={{ gap: 10, alignItems: "flex-start" }}>
            <p className="muted grow" style={{ margin: 0 }}>
              Without it, BYTE finds passages by their words. With it ({bytes(kb?.embedBytes ?? 0)} download), questions worded differently
              from your files still find the right passage.
            </p>
            {kb?.embedKey && (
              <button className="btn sm" disabled={!!embedBusy} onClick={() => void guard(() => api.modelDownload(kb.embedKey!))}>
                {embedBusy ? <Loader2 size={14} className="spin" /> : <Sparkles size={14} />}
                {embedBusy && embedDl && embedDl.total > 0 ? `${Math.round((embedDl.bytes / embedDl.total) * 100)}%` : "Download"}
              </button>
            )}
          </div>
        )}
        <label className="row" style={{ gap: 10, cursor: "pointer", marginTop: 12 }}>
          <input type="checkbox" checked={settings.answerCache} onChange={(e) => void update({ answerCache: e.target.checked })} />
          <span className="grow">
            <b>Instant answers</b>
            <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
              When a new chat asks almost exactly what you asked in the last week, BYTE shows that answer at once (marked, with
              Regenerate for a fresh one). Never for questions about the news, prices or anything else that changes.
              {!kb?.embedInstalled && " Needs search by meaning."}
            </span>
          </span>
          <button className="btn sm ghost" onClick={(e) => (e.preventDefault(), void guard(() => api.answerCacheClear()))} title="Forget saved answers">
            Clear
          </button>
        </label>
      </div>
    </>
  );
}

function SourceRow({ s, busy, onReindex, onRemove }: { s: KbSource; busy: boolean; onReindex(): void; onRemove(): void }) {
  return (
    <div className="kb-source">
      <div className="grow" style={{ minWidth: 0 }}>
        <div className="name" title={s.path}>{shortPath(s.path)}</div>
        <div className="faint" style={{ fontSize: "0.82em" }}>
          {s.files} files · {s.chunks} passages
          {s.chunks > 0 && s.embedded < s.chunks ? ` (${s.embedded} searchable by meaning)` : ""} · checked {ago(s.lastScan)}
        </div>
        {s.error && <div className="attach-error" style={{ fontSize: "0.82em" }}>{s.error}</div>}
      </div>
      <button className="icon-btn" onClick={onReindex} disabled={busy} title="Check this folder for changes">
        <RefreshCw size={14} />
      </button>
      <button className="icon-btn" onClick={onRemove} title="Remove this folder (your files aren't touched)">
        <Trash2 size={14} />
      </button>
    </div>
  );
}
