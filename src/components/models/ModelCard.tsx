import { openUrl } from "@tauri-apps/plugin-opener";
import { Brain, ChevronDown, CircleCheck, Clock, Download, ExternalLink, Eye, Gauge, Layers, Pause, Play, Sparkles, Trash2, TriangleAlert, Users, Wrench } from "lucide-react";
import { useEffect, useState } from "react";

import { bytes, contextLabel, eta } from "../../lib/format";
import { approxDuration, paramsLabel, quantLabel, shortQuant, speedClass, TAG_LABELS } from "../../lib/models";
import type { LoadedModel, ModelDetails, ModelStatus, VariantStatus } from "../../lib/types";
import type { DownloadState } from "../../state/store";

/** How well one version fits this Mac. */
export function FitPill({ v }: { v: VariantStatus }) {
  const f = v.fit;
  if (f.fit === "toobig") return <span className="pill danger" title={f.note}><TriangleAlert size={12} /> Needs {v.minRamGb} GB</span>;
  if (f.fit === "tight") return <span className="pill warn" title={f.note}>Fits · {contextLabel(f.context)} context</span>;
  return <span className="pill ok" title={f.note}>Great fit · {contextLabel(f.context)} context</span>;
}

const STRENGTH_LABELS: [string, string][] = [
  ["chat", "Conversation"],
  ["writing", "Writing"],
  ["coding", "Coding"],
  ["reasoning", "Reasoning"],
  ["math", "Math"],
  ["languages", "Other languages"],
  ["speed", "Speed"],
];

/** The dropdown: what the model is, who made it, what it's good at, ideas. */
export function ModelDetailsView({ d, model }: { d: ModelDetails; model: ModelStatus }) {
  return (
    <div className="model-details">
      {d.about && <p>{d.about}</p>}
      <div className="details-meta">
        {d.author && (
          <span>
            <span className="faint">Made by</span> <b>{d.author}</b>
          </span>
        )}
        {model.released && <span className="faint">Released {model.released}</span>}
        {model.license && <span className="faint">{model.license}</span>}
        {d.sourceUrl && (
          <button className="linklike" onClick={() => void openUrl(d.sourceUrl!)}>
            Model page <ExternalLink size={11} />
          </button>
        )}
      </div>
      {d.caution && (
        <div className="details-caution">
          <TriangleAlert size={13} /> {d.caution}
        </div>
      )}
      <div className="strengths" aria-label="How good it is at different things">
        {STRENGTH_LABELS.filter(([k]) => d.strengths[k]).map(([k, label]) => (
          <div key={k} className="strength">
            <span>{label}</span>
            <span className="bar" aria-label={`${d.strengths[k]} of 5`}>
              {[1, 2, 3, 4, 5].map((i) => (
                <i key={i} className={i <= d.strengths[k] ? "on" : ""} />
              ))}
            </span>
          </div>
        ))}
        <span className="faint strengths-note">BYTE's estimate from the model's size, family and card.</span>
      </div>
      {d.ideas.length > 0 && (
        <div>
          <span className="faint">Try it for</span>
          <ul className="ideas">
            {d.ideas.map((i) => (
              <li key={i}>{i}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
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
  /** Models in memory, for "Load alongside" / "Unload". */
  loaded?: LoadedModel[];
  onLoad?(key: string): void;
  onUnload?(key: string): void;
}

/** The image reader (mmproj) that lets a model see photos: a separate download. */
function VisionRow({
  vision,
  dl,
  onDownload,
  onPause,
  onDelete,
}: {
  vision: NonNullable<ModelStatus["vision"]>;
  dl?: DownloadState;
  onDownload(key: string): void;
  onPause(key: string): void;
  onDelete?(key: string): void;
}) {
  const busy = vision.downloading || dl?.phase === "downloading" || dl?.phase === "resuming" || dl?.phase === "verifying";
  return (
    <div className="vision-row">
      <Eye size={13} />
      {vision.installed ? (
        <span className="grow">Sees photos: attach one in the chat box.</span>
      ) : busy ? (
        <span className="grow">
          Downloading the image reader… {dl && dl.total > 0 ? `${Math.round((dl.bytes / dl.total) * 100)}%` : ""}
        </span>
      ) : (
        <span className="grow faint">Add the image reader so this model can look at photos you attach.</span>
      )}
      {!vision.installed && !busy && (
        <button className="btn sm" onClick={() => onDownload(vision.key)}>
          <Download size={14} /> {dl?.phase === "paused" || dl?.phase === "failed" ? "Resume" : `Image reader ${bytes(vision.sizeBytes)}`}
        </button>
      )}
      {busy && dl?.phase !== "verifying" && (
        <button className="btn sm" onClick={() => onPause(vision.key)}>
          <Pause size={14} /> Pause
        </button>
      )}
      {vision.installed && onDelete && (
        <button className="icon-btn" onClick={() => onDelete(vision.key)} title="Delete the image reader">
          <Trash2 size={15} />
        </button>
      )}
    </div>
  );
}

/** A catalog model with a version picker, fit for this Mac, and actions. */
export function ModelCard({ model, recommended, downloads, activeKey, onDownload, onPause, onDelete, onActivate, loaded = [], onLoad, onUnload }: Props) {
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
  const extra = loaded.find((l) => l.key === v.key && !l.primary);
  const canChat = v.installed && model.role === "chat";
  const [open, setOpen] = useState(false);
  const d = model.details;

  return (
    <div className={`model-card ${active ? "active" : ""}`}>
      <div className="row" style={{ alignItems: "flex-start" }}>
        <div className="grow">
          <div className="title">
            {model.name}
            {paramsLabel(model) && <span className="faint" style={{ fontWeight: 500, fontSize: "0.85em" }}>{paramsLabel(model)}</span>}
            {isPick && <span className="pill accent"><Sparkles size={11} /> BYTE's pick</span>}
            {d?.community && <span className="pill" title="Made by the community, not the original model's maker"><Users size={11} /> Community</span>}
            {active && <span className="pill ok"><CircleCheck size={12} /> In use</span>}
            {extra && (
              <span className="pill accent" title="Loaded alongside the main model">
                <Layers size={11} /> {extra.status.state === "starting" ? "Loading…" : "Loaded"}
              </span>
            )}
          </div>
          <div className="muted" style={{ fontSize: "0.92em" }}>{model.tagline}</div>
          {model.usedFor && (
            <div style={{ fontSize: "0.88em", marginTop: 2 }}>
              <span className="faint">Good for:</span> {model.usedFor}
              {d?.author && <span className="faint"> · by {d.author}</span>}
            </div>
          )}
        </div>
        <div className="row" style={{ gap: 6, flex: "none" }}>
          {canChat && !active && !extra && v.fitsAlongside && onLoad && (
            <button className="btn sm" onClick={() => onLoad(v.key)} title="Keep this model in memory next to the main one, to switch instantly or compare answers">
              <Layers size={14} /> Load alongside
            </button>
          )}
          {extra && onUnload && (
            <button className="btn sm ghost" onClick={() => onUnload(v.key)}>Unload</button>
          )}
          {canChat && !active && onActivate && (
            <button className="btn sm primary" onClick={() => onActivate(v.key)} disabled={tooBig} title="Make this the main model">Use</button>
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
        {model.vision && (
          <span className="tag" title="Can look at photos you attach, once its image reader is downloaded">
            <Eye size={11} /> Sees images
          </span>
        )}
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
        <div
          className={`speed-row ${speedClass(v.measuredTps ?? v.speed.tokensPerSec)}`}
          title={v.measuredTps ? "Measured on this Mac by tuning, with its fastest settings." : "Estimated from this Mac's chip; actual speed varies with prompt length and other apps."}
        >
          {v.measuredTps ? (
            <span><Gauge size={13} /> {v.measuredTps.toFixed(1)} tokens/sec measured on this Mac</span>
          ) : (
            <span><Gauge size={13} /> ≈ {Math.round(v.speed.tokensPerSec)} tokens/sec on this Mac</span>
          )}
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
      {model.vision && model.role === "chat" && model.variants.some((x) => x.installed) && (
        <VisionRow vision={model.vision} dl={downloads[model.vision.key]} onDownload={onDownload} onPause={onPause} onDelete={onDelete} />
      )}
      {d && (
        <>
          <button className="details-toggle" onClick={() => setOpen(!open)} aria-expanded={open}>
            <ChevronDown size={14} style={{ transform: open ? "rotate(180deg)" : undefined }} /> {open ? "Hide details" : "Details: what it's good at, who made it, ideas"}
          </button>
          {open && <ModelDetailsView d={d} model={model} />}
        </>
      )}
    </div>
  );
}
