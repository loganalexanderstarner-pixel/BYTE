import { CircleCheck, Download, Pause, Play, Trash2, TriangleAlert } from "lucide-react";

import { bytes, contextLabel, eta } from "../../lib/format";
import type { ModelStatus } from "../../lib/types";
import type { DownloadState } from "../../state/store";

export function FitPill({ model }: { model: ModelStatus }) {
  const f = model.fit;
  if (f.fit === "toobig") return <span className="pill danger" title={f.note}><TriangleAlert size={12} /> Too big for this Mac</span>;
  if (f.fit === "tight") return <span className="pill warn" title={f.note}>Fits · {contextLabel(f.context)} context</span>;
  return <span className="pill ok" title={f.note}>Great fit · {contextLabel(f.context)} context</span>;
}

export function DownloadProgress({ model, dl }: { model: ModelStatus; dl?: DownloadState }) {
  const total = dl?.total || model.sizeBytes;
  const done = dl?.bytes ?? model.partialBytes;
  const pct = total ? Math.min(100, (done / total) * 100) : 0;
  let label: string;
  // A partial file on disk with no live download means it was paused earlier.
  const phase = dl?.phase ?? (done > 0 ? "paused" : undefined);
  switch (phase) {
    case "resuming":
      label = "Checking the part already downloaded…";
      break;
    case "verifying":
      label = "Verifying the file…";
      break;
    case "paused":
      label = `Paused · ${bytes(done)} of ${bytes(total)}`;
      break;
    case "failed":
      label = dl?.error ?? "Download failed";
      break;
    case "finished":
      label = "Downloaded";
      break;
    default:
      label = `${bytes(done)} of ${bytes(total)} · ${dl?.bytesPerSec ? `${bytes(dl.bytesPerSec)}/s · ${eta(done, total, dl.bytesPerSec)} left` : "starting…"}`;
  }
  return (
    <div>
      <div className="progress" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
        <span style={{ width: `${dl?.phase === "verifying" || dl?.phase === "finished" ? 100 : pct}%` }} />
      </div>
      <div className={dl?.phase === "failed" ? "" : "faint"} style={{ marginTop: 6, fontSize: "0.85em", color: dl?.phase === "failed" ? "var(--danger)" : undefined }}>
        {label}
      </div>
    </div>
  );
}

interface Props {
  model: ModelStatus;
  dl?: DownloadState;
  active?: boolean;
  onDownload(): void;
  onPause(): void;
  onDelete?(): void;
  onActivate?(): void;
}

export function ModelCard({ model, dl, active, onDownload, onPause, onDelete, onActivate }: Props) {
  const downloading = model.downloading || dl?.phase === "downloading" || dl?.phase === "resuming" || dl?.phase === "verifying";
  const hasPartial = !model.installed && (model.partialBytes > 0 || dl?.phase === "paused" || dl?.phase === "failed");
  const tooBig = model.fit.fit === "toobig";
  return (
    <div className={`model-card ${active ? "active" : ""}`}>
      <div className="row">
        <div className="grow">
          <div className="title">
            {model.name}
            {model.recommended && <span className="pill accent">Recommended</span>}
            {active && <span className="pill ok"><CircleCheck size={12} /> In use</span>}
          </div>
          <div className="muted" style={{ fontSize: "0.92em" }}>{model.tagline}</div>
        </div>
        <div className="row" style={{ gap: 6 }}>
          {model.installed && model.role === "chat" && !active && onActivate && (
            <button className="btn sm primary" onClick={onActivate} disabled={tooBig}>Use</button>
          )}
          {!model.installed && !downloading && (
            <button className="btn sm" onClick={onDownload} disabled={tooBig} title={tooBig ? model.fit.note : undefined}>
              {hasPartial ? <Play size={14} /> : <Download size={14} />}
              {hasPartial ? "Resume" : "Download"}
            </button>
          )}
          {downloading && dl?.phase !== "verifying" && (
            <button className="btn sm" onClick={onPause}><Pause size={14} /> Pause</button>
          )}
          {(model.installed || hasPartial) && onDelete && !downloading && (
            <button className="icon-btn" onClick={onDelete} title={model.installed ? "Delete model" : "Discard partial download"}>
              <Trash2 size={15} />
            </button>
          )}
        </div>
      </div>
      <div className="meta">
        <span>{bytes(model.sizeBytes)}</span>
        {model.role === "chat" && <span>· {model.speedHint} on M4</span>}
        {model.thinking && <span>· Thinking</span>}
        {model.role !== "chat" && <span>· Helper model</span>}
        <span style={{ marginLeft: "auto" }}><FitPill model={model} /></span>
      </div>
      {tooBig && <div className="faint" style={{ fontSize: "0.85em" }}>{model.fit.note}</div>}
      {(downloading || hasPartial) && <DownloadProgress model={model} dl={dl} />}
    </div>
  );
}
