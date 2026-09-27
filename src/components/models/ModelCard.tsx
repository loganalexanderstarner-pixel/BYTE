import { Brain, CircleCheck, Clock, Download, Gauge, Pause, Play, Sparkles, Trash2, TriangleAlert, Wrench } from "lucide-react";
import { useEffect, useState } from "react";

import { bytes, contextLabel, eta } from "../../lib/format";
import { approxDuration, paramsLabel, quantLabel, shortQuant, speedClass, TAG_LABELS } from "../../lib/models";
import type { ModelStatus, VariantStatus } from "../../lib/types";
import type { DownloadState } from "../../state/store";

/** How well one version fits this Mac. */
export function FitPill({ v }: { v: VariantStatus }) {
  const f = v.fit;
  if (f.fit === "toobig") return <span className="pill danger" title={f.note}><TriangleAlert size={12} /> Needs {v.minRamGb} GB</span>;
  if (f.fit === "tight") return <span className="pill warn" title={f.note}>Fits · {contextLabel(f.context)} context</span>;
  return <span className="pill ok" title={f.note}>Great fit · {contextLabel(f.context)} context</span>;
}

export function DownloadProgress({ variant, dl }: { variant: VariantStatus; dl?: DownloadState }) {
  const total = dl?.total || variant.sizeBytes;
  const done = dl?.bytes ?? variant.partialBytes;
  const pct = total ? Math.min(100, (done / total) * 100) : 0;
  // A partial file on disk with no live download means it was paused earlier.
  const phase = dl?.phase ?? (done > 0 ? "paused" : undefined);
  let label: string;
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
  const full = phase === "verifying" || phase === "finished";
  return (
    <div>
      <div className="progress" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
        <span style={{ width: `${full ? 100 : pct}%` }} />
      </div>
      <div className="faint" style={{ marginTop: 6, fontSize: "0.85em", color: phase === "failed" ? "var(--danger)" : undefined }}>
        {label}
      </div>
    </div>
  );
}

interface Props {
  model: ModelStatus;
  /** The overall recommendation for this Mac ("model:quant"). */
  recommended?: string | null;
  downloads: Record<string, DownloadState>;
  activeKey?: string | null;
  onDownload(key: string): void;
  onPause(key: string): void;
  onDelete?(key: string): void;
  onActivate?(key: string): void;
}

/** A catalog model with a version picker, fit for this Mac, and actions. */
export function ModelCard({ model, recommended, downloads, activeKey, onDownload, onPause, onDelete, onActivate }: Props) {
  const initial =
    model.variants.find((v) => v.key === activeKey) ??
    model.variants.find((v) => v.downloading || v.partialBytes > 0) ??
    model.variants.find((v) => v.key === model.best) ??
    model.variants[0];
  const [selected, setSelected] = useState(initial.key);
  useEffect(() => {
    if (!model.variants.some((v) => v.key === selected)) setSelected(initial.key);
  }, [model.variants, selected, initial.key]);

  const v = model.variants.find((x) => x.key === selected) ?? initial;
  const dl = downloads[v.key];
  const downloading = v.downloading || dl?.phase === "downloading" || dl?.phase === "resuming" || dl?.phase === "verifying";
  const hasPartial = !v.installed && (v.partialBytes > 0 || dl?.phase === "paused" || dl?.phase === "failed");
  const tooBig = v.fit.fit === "toobig";
  const active = activeKey === v.key && v.installed;
  const installedOthers = model.variants.filter((x) => x.installed && x.key !== v.key);
  const isPick = !!recommended && model.variants.some((x) => x.key === recommended);

  return (
    <div className={`model-card ${active ? "active" : ""}`}>
      <div className="row" style={{ alignItems: "flex-start" }}>
        <div className="grow">
          <div className="title">
            {model.name}
            {paramsLabel(model) && <span className="faint" style={{ fontWeight: 500, fontSize: "0.85em" }}>{paramsLabel(model)}</span>}
            {isPick && <span className="pill accent"><Sparkles size={11} /> BYTE's pick</span>}
            {active && <span className="pill ok"><CircleCheck size={12} /> In use</span>}
          </div>
          <div className="muted" style={{ fontSize: "0.92em" }}>{model.tagline}</div>
          {model.usedFor && (
            <div style={{ fontSize: "0.88em", marginTop: 2 }}>
              <span className="faint">Good for:</span> {model.usedFor}
            </div>
          )}
        </div>
        <div className="row" style={{ gap: 6, flex: "none" }}>
          {v.installed && model.role === "chat" && !active && onActivate && (
            <button className="btn sm primary" onClick={() => onActivate(v.key)} disabled={tooBig}>Use</button>
          )}
          {!v.installed && !downloading && (
            <button className="btn sm" onClick={() => onDownload(v.key)} disabled={tooBig} title={tooBig ? v.fit.note : undefined}>
              {hasPartial ? <Play size={14} /> : <Download size={14} />}
              {hasPartial ? "Resume" : `Download ${bytes(v.sizeBytes)}`}
            </button>
          )}
          {downloading && dl?.phase !== "verifying" && (
            <button className="btn sm" onClick={() => onPause(v.key)}><Pause size={14} /> Pause</button>
          )}
          {(v.installed || hasPartial) && onDelete && !downloading && (
            <button className="icon-btn" onClick={() => onDelete(v.key)} title={v.installed ? "Delete this version" : "Discard partial download"}>
              <Trash2 size={15} />
            </button>
          )}
        </div>
      </div>

      <div className="meta">
        {model.tags.filter((t) => TAG_LABELS[t]).map((t) => (
          <span key={t} className="tag">{TAG_LABELS[t]}</span>
        ))}
        {model.thinking && <span className="tag"><Brain size={11} /> Thinking</span>}
        {model.tools && <span className="tag"><Wrench size={11} /> Tools</span>}
        {model.family && <span className="faint">· {model.family}</span>}
        {model.released && <span className="faint">· {model.released}</span>}
        {model.license && <span className="faint">· {model.license}</span>}
      </div>

      <div className="variant-row">
        <label className="faint" htmlFor={`v-${model.id}`}>Version</label>
        <select id={`v-${model.id}`} value={v.key} onChange={(e) => setSelected(e.target.value)}>
          {model.variants.map((x) => (
            <option key={x.key} value={x.key}>
              {shortQuant(x.quant)} · {bytes(x.sizeBytes)} · {quantLabel(x.bits)}
              {x.fit.fit === "toobig" ? ` · needs ${x.minRamGb} GB` : ""}
              {x.installed ? " · downloaded" : ""}
              {x.key === model.best ? " · best for this Mac" : ""}
            </option>
          ))}
        </select>
        <span style={{ marginLeft: "auto" }}>
          <FitPill v={v} />
        </span>
      </div>
      {!tooBig && (
        <div className={`speed-row ${speedClass(v.speed.tokensPerSec)}`} title="Estimated from this Mac's chip; actual speed varies with prompt length and other apps.">
          <span><Gauge size={13} /> ≈ {Math.round(v.speed.tokensPerSec)} tokens/sec on this Mac</span>
          <span><Clock size={13} /> Typical answer {approxDuration(v.speed.replySecs)}</span>
          {model.thinking && <span className="faint">({approxDuration(v.speed.replyThinkingSecs)} with thinking)</span>}
        </div>
      )}
      {tooBig && <div className="faint" style={{ fontSize: "0.85em" }}>{v.fit.note}</div>}
      {installedOthers.length > 0 && (
        <div className="faint" style={{ fontSize: "0.85em" }}>
          Also downloaded: {installedOthers.map((x) => shortQuant(x.quant)).join(", ")}
        </div>
      )}
      {(downloading || hasPartial) && <DownloadProgress variant={v} dl={dl} />}
    </div>
  );
}
