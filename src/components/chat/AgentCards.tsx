import { AppWindow, Check, Download, ExternalLink, FileText, FolderOpen, Image as ImageIcon, Send, ShieldAlert, X } from "lucide-react";
import { useState } from "react";

import { approveLabel, canOpen, fileSize } from "../../lib/agent";
import { api, errorText } from "../../lib/api";
import type { ApprovalCard as Card, SavedFile } from "../../lib/types";

/** BYTE wants to submit, commit or download: nothing happens until the user says so. */
export function ApprovalCard({ card }: { card: Card }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const decide = async (ok: boolean) => {
    setBusy(true);
    try {
      const waiting = await api.agentApprove(card.id, ok);
      if (!waiting) setError("This request isn't waiting any more.");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const Icon = card.action === "download" ? Download : Send;
  return (
    <div className={`approval ${card.status}`} role="region" aria-label="Needs your OK">
      <div className="approval-head">
        <ShieldAlert size={16} />
        <b>{card.title}</b>
      </div>
      {card.fields.length > 0 && (
        <dl className="approval-fields">
          {card.fields.map((f, i) => (
            <div key={i}>
              <dt>{f.label}</dt>
              <dd>{f.value || <span className="muted">(empty)</span>}</dd>
            </div>
          ))}
        </dl>
      )}
      <div className="approval-site">
        {card.action === "download" ? "From" : "On"} <span className="mono">{card.url}</span>
      </div>
      {card.status === "waiting" ? (
        <div className="approval-actions">
          <button className="btn sm primary" disabled={busy} onClick={() => void decide(true)}>
            <Icon size={13} /> {approveLabel(card)}
          </button>
          <button className="btn sm ghost" disabled={busy} onClick={() => void decide(false)}>
            <X size={13} /> Don't
          </button>
          <button className="btn sm ghost" onClick={() => void api.agentShow(true)} title="See the page BYTE is using">
            <AppWindow size={13} /> Show the page
          </button>
          {error && <span className="hint danger">{error}</span>}
        </div>
      ) : (
        <div className="approval-result">
          {card.status === "approved" ? (
            <>
              <Check size={13} /> You approved this
            </>
          ) : card.status === "declined" ? (
            <>
              <X size={13} /> You said no, so nothing was sent
            </>
          ) : (
            <>
              <X size={13} /> Not answered, so nothing was sent
            </>
          )}
        </div>
      )}
    </div>
  );
}

const ICONS = { download: Download, pdf: FileText, image: ImageIcon, archive: FileText, text: FileText } as const;

/** Files the web agent saved (downloads, pages as PDF or pictures). */
export function SavedFiles({ files }: { files: SavedFile[] }) {
  const [error, setError] = useState<string | null>(null);
  const act = (f: SavedFile, open: boolean) => void api.agentFile(f.path, open).catch((e) => setError(errorText(e)));
  return (
    <div className="saved-files">
      {files.map((f) => {
        const Icon = ICONS[f.format] ?? FileText;
        return (
          <span key={f.path} className="saved-file">
            <Icon size={14} />
            <span className="name" title={f.path}>
              {f.name}
            </span>
            <span className="muted">{fileSize(f.bytes)}</span>
            {canOpen(f) && (
              <button className="icon-btn sm" onClick={() => act(f, true)} title="Open" aria-label={`Open ${f.name}`}>
                <ExternalLink size={13} />
              </button>
            )}
            <button className="icon-btn sm" onClick={() => act(f, false)} title="Show in Finder" aria-label={`Show ${f.name} in Finder`}>
              <FolderOpen size={13} />
            </button>
          </span>
        );
      })}
      {error && <span className="hint danger">{error}</span>}
    </div>
  );
}

/** While BYTE browses: watch it, or take over (a login, a CAPTCHA). */
export function BrowsingBar() {
  const [shown, setShown] = useState(false);
  const toggle = async () => {
    const ok = await api.agentShow(!shown).catch(() => false);
    if (ok) setShown(!shown);
  };
  return (
    <div className="browsing-bar">
      <span className="pulse" aria-hidden />
      BYTE is using a private browser
      <button className="btn sm ghost" onClick={() => void toggle()}>
        <AppWindow size={13} /> {shown ? "Hide browser" : "Show browser"}
      </button>
    </div>
  );
}
