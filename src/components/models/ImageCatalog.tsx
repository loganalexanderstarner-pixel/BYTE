import { Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { api, errorText } from "../../lib/api";
import { bytes } from "../../lib/format";
import { fitSentence, PLACEMENT_LABEL, placementClass } from "../../lib/imageFit";
import type { ImageModel } from "../../lib/types";
import { useStore } from "../../state/store";

/** What a download is doing right now (from the engine's events). */
export interface DlState {
  bytes: number;
  total: number;
  phase: "downloading" | "verifying" | "paused" | "failed" | "done";
  message?: string;
}

export function toDl(e: import("../../lib/types").DownloadEvent, prev?: DlState): DlState {
  switch (e.kind) {
    case "progress":
      return { bytes: e.bytes, total: e.total, phase: "downloading" };
    case "resuming":
      return { bytes: e.bytes, total: prev?.total ?? 0, phase: "downloading" };
    case "verifying":
      return { bytes: prev?.total ?? 0, total: prev?.total ?? 0, phase: "verifying" };
    case "paused":
      return { bytes: e.bytes, total: prev?.total ?? 0, phase: "paused" };
    case "failed":
      return { bytes: prev?.bytes ?? 0, total: prev?.total ?? 0, phase: "failed", message: e.message };
    case "finished":
      return { bytes: prev?.total ?? 0, total: prev?.total ?? 0, phase: "done" };
  }
}

type Sort = "best" | "fastest" | "smallest" | "popular";

/** Every kind of image job. Editing jobs are done by the edit models (Qwen-Image Edit, FLUX Kontext, FLUX.2 klein). */
export const TASKS: { id: string; label: string; hint: string }[] = [
  { id: "generate", label: "Make pictures", hint: "Make a new picture from words" },
  { id: "edit", label: "Edit a photo", hint: "Change a photo with a sentence: lighting, outfit, background, text on a sign" },
  { id: "remove", label: "Remove things", hint: "Take a person or object out of a photo" },
  { id: "upscale", label: "Upscale", hint: "Make a picture bigger and sharper, fix zoom pixelation" },
  { id: "restore", label: "Restore faces and old photos", hint: "Sharpen faces, fix scratches, colorize" },
  { id: "background", label: "Remove background", hint: "Cut a subject out" },
  { id: "style", label: "Change the style", hint: "Turn a photo into a painting, anime or sketch" },
];

/** Whether a model can do a job: editing jobs use the edit models, and some models are listed for a job by their tags. */
export function forTask(m: ImageModel, task: string): boolean {
  switch (task) {
    case "generate":
    case "upscale":
      return m.kind === task;
    case "restore":
    case "style":
      return m.tags.includes(task);
    default:
      return m.kind === "edit" || m.tags.includes("edit");
  }
}

/** Real-life equivalents offered under Realism. */
export const REAL_LOOKS: { id: string; label: string }[] = [
  { id: "portrait", label: "Portraits and people" },
  { id: "street", label: "Street and everyday life" },
  { id: "landscape", label: "Landscapes and nature" },
  { id: "animals", label: "Animals and wildlife" },
  { id: "product", label: "Product photos" },
  { id: "food", label: "Food photos" },
  { id: "architecture", label: "Buildings and interiors" },
  { id: "cinematic", label: "Movie-still look" },
  { id: "film", label: "Film and vintage photo look" },
];

export const IMAGE_FILTERS: { id: string; label: string; hint?: string }[] = [
  { id: "realism", label: "Realism", hint: "Photo-like people, places and objects" },
  { id: "cartoon", label: "Cartoon", hint: "Cartoon, anime and flat colour styles" },
  { id: "illustration", label: "Illustration", hint: "Paintings, concept art and book art" },
  { id: "poster", label: "Poems and posters", hint: "Good for art behind text: poem posters, covers, invitations" },
  { id: "text", label: "Words in pictures", hint: "Better at spelling out words inside the image" },
  { id: "fast", label: "Fast" },
  { id: "lowvram", label: "Small graphics card", hint: "Runs on 6 GB or less of video memory" },
];


/** What a model stands out for: shown on its card whatever filter is on, so nothing is hidden by a filter. */
export function goodAt(m: ImageModel): string[] {
  const out: string[] = [];
  for (const t of m.tags) {
    const f = IMAGE_FILTERS.find((x) => x.id === t);
    if (f && f.id !== "realism") out.push(f.label);
  }
  if (m.tags.includes("realism")) {
    const looks = m.looks.slice(0, 3).map((k) => REAL_LOOKS.find((l) => l.id === k)?.label ?? k);
    out.unshift(...(looks.length ? looks.map((l) => `Realistic: ${l.toLowerCase()}`) : ["Realism"]));
  }
  return out.slice(0, 6);
}

/** Quality of the version recommended for this PC (quant losses included). */
function headline(m: ImageModel): number {
  return m.quality - (m.variants[m.best]?.qualityCost ?? 0);
}

/** Sorts and filters the image catalog. Pure, so it is tested without the UI. */
export function arrange(models: ImageModel[], opts: { query: string; filter: string | null; look?: string | null; task?: string | null; sort: Sort; community: boolean; commercialOnly: boolean; kids?: boolean }): ImageModel[] {
  const q = opts.query.trim().toLowerCase();
  // Community models need their own switch; adult ones are never shown in kids mode.
  let list = models.filter((m) => (opts.community || !m.community) && !(opts.kids && m.mature));
  if (opts.commercialOnly) list = list.filter((m) => m.commercialOk);
  if (opts.task) list = list.filter((m) => forTask(m, opts.task!));
  if (opts.filter) list = list.filter((m) => m.tags.includes(opts.filter!));
  if (opts.filter === "realism" && opts.look) list = list.filter((m) => m.looks.includes(opts.look!));
  if (q) list = list.filter((m) => [m.name, m.family, m.tagline, m.strengths, m.tags.join(" ")].join(" ").toLowerCase().includes(q));
  const by: Record<Sort, (a: ImageModel, b: ImageModel) => number> = {
    best: (a, b) => headline(b) - headline(a),
    fastest: (a, b) => a.steps - b.steps || headline(b) - headline(a),
    smallest: (a, b) => a.totalBytes - b.totalBytes,
    popular: (a, b) => b.downloads - a.downloads,
  };
  return [...list].sort(by[opts.sort]);
}

/**
 * Image Studio's model list (Windows builds): what each free image model is good at and whether this
 * PC's graphics card can run it. Downloading and making pictures arrive in the next builds.
 */
export function ImageCatalog({ models, reload }: { models: ImageModel[] | null; reload: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const [dl, setDl] = useState<Record<string, DlState>>({});
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<string | null>(null);
  const [look, setLook] = useState<string | null>(null);
  const [task, setTask] = useState<string | null>(null);
  const [sort, setSort] = useState<Sort>("best");
  const [community, setCommunity] = useState(false);
  const [commercialOnly, setCommercialOnly] = useState(false);
  const [pick, setPick] = useState<Record<string, number>>({});
  const [fitOnly, setFitOnly] = useState(false);
  const kids = useStore((s) => !!s.settings?.kidsMode);

  useEffect(() => {
    let off: (() => void) | undefined;
    void api.onImageDownload((e) => {
      setDl((d) => ({ ...d, [e.id]: toDl(e, d[e.id]) }));
      if (e.kind === "finished" || e.kind === "paused") reload();
    }).then((u) => (off = u));
    return () => off?.();
  }, [reload]);
  const run = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    reload();
  };

  const shown = useMemo(() => arrange(models ?? [], { query, filter, look, task, sort, community, commercialOnly, kids }).filter((m) => !fitOnly || m.variants.some((v) => v.fit.placement === "vram")), [models, query, filter, look, task, sort, community, commercialOnly, kids, fitOnly]);

  return (
    <>
      <p className="faint" style={{ marginTop: 0 }}>
        Free image models, best first. Everything runs on this PC; pictures and photos never leave it. Download a version, then use Make a picture at the top of this page.
      </p>
      <div className="catalog-tools">
        <label className="search">
          <Search size={14} className="faint" />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search image models (e.g. realistic, fast, FLUX)" aria-label="Search image models" />
        </label>
        <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort">
          <option value="best">Best to least</option>
          <option value="fastest">Fewest steps (fastest)</option>
          <option value="popular">Most downloaded</option>
          <option value="smallest">Smallest download</option>
        </select>
      </div>
      <div className="filters">
        <div className="chips" role="group" aria-label="What you want to do">
          <button className="chip" aria-pressed={task === null} onClick={() => setTask(null)}>Everything</button>
          {TASKS.map((t) => {
            const n = (models ?? []).filter((m) => forTask(m, t.id)).length;
            return (
              <button key={t.id} className="chip" disabled={n === 0} aria-pressed={task === t.id} onClick={() => setTask(task === t.id ? null : t.id)} title={n === 0 ? `${t.hint}. Models for this are coming.` : t.hint}>
                {t.label}{n === 0 ? " (coming)" : ` (${n})`}
              </button>
            );
          })}
        </div>
        <div className="chips" role="group" aria-label="Style">
          <button className="chip" aria-pressed={filter === null} onClick={() => setFilter(null)}>All</button>
          {IMAGE_FILTERS.map((f) => (
            <button key={f.id} className="chip" title={f.hint} aria-pressed={filter === f.id} onClick={() => (setFilter(filter === f.id ? null : f.id), setLook(null))}>
              {f.label}
            </button>
          ))}
        </div>
        {filter === "realism" && (
          <div className="chips" role="group" aria-label="Real-life look">
            <span className="faint small" style={{ alignSelf: "center" }}>Real-life look:</span>
            {REAL_LOOKS.map((l) => (
              <button key={l.id} className="chip" aria-pressed={look === l.id} onClick={() => setLook(look === l.id ? null : l.id)}>
                {l.label}
              </button>
            ))}
          </div>
        )}
        <div className="chips" role="group" aria-label="Options">
          <button className="chip" aria-pressed={fitOnly} onClick={() => setFitOnly(!fitOnly)} title="Only models with a version that fits entirely in your graphics card's memory">
            Fits my graphics card
          </button>
          <button className="chip" aria-pressed={commercialOnly} onClick={() => setCommercialOnly(!commercialOnly)} title="Hide models whose licence does not allow selling what you make">
            Commercial use OK
          </button>
          <button className="chip" aria-pressed={community} onClick={() => setCommunity(!community)} title="Models made by the community rather than the original labs. Check each licence.">
            Community models
          </button>
        </div>
      </div>
      {error && <div className="banner danger">{error}</div>}
      {models && models.length === 0 && <p className="faint">Image models are only in the Windows build for now.</p>}
      {models && models.length > 0 && shown.length === 0 && <p className="faint">No image models match these filters.</p>}
      <div className="model-list" style={{ marginTop: 12 }}>
        {shown.map((m) => {
          const vi = pick[m.id] ?? m.best;
          const v = m.variants[vi];
          return (
            <div key={m.id} className="model-card">
              <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                <b className="grow">{m.name}</b>
                <span className="faint">{m.family}</span>
                <span className={`pill ${placementClass(v.fit.placement)}`}>{PLACEMENT_LABEL[v.fit.placement]}</span>
              </div>
              <div>{m.tagline}</div>
              <div className="row" style={{ gap: 6, flexWrap: "wrap" }} aria-label="Good at">
                {goodAt(m).map((g) => (
                  <span key={g} className="pill ok">{g}</span>
                ))}
              </div>
              <div className="faint" style={{ fontSize: "0.9em" }}>{m.strengths}</div>
              <div className="row" style={{ gap: 8, flexWrap: "wrap" }} role="group" aria-label={`Version of ${m.name}`}>
                {m.variants.map((x, i) => (
                  <button
                    key={x.quant}
                    className="chip"
                    aria-pressed={i === vi}
                    onClick={() => setPick({ ...pick, [m.id]: i })}
                    title={`${x.quant}: ${bytes(x.sizeBytes)}${x.qualityCost ? `, about ${x.qualityCost} points less quality` : ", best quality"}`}
                  >
                    {x.quant} · {bytes(x.sizeBytes)}{i === m.best ? " ★" : ""}
                  </button>
                ))}
              </div>
              <div className={placementClass(v.fit.placement) === "danger" ? "banner danger" : "faint"} style={{ fontSize: "0.9em" }}>
                {fitSentence(v)}{vi === m.best ? " Recommended for this PC." : ""}
              </div>
              <DownloadRow m={m} vi={vi} state={dl[`${m.id}:${v.quant}`]} onDownload={() => run(() => api.imageDownload(m.id, v.quant))} onPause={() => run(() => api.imagePause(m.id, v.quant))} onDelete={() => run(() => api.imageDelete(m.id, v.quant))} />
              <div className="faint row" style={{ gap: 12, flexWrap: "wrap", fontSize: "0.85em" }}>
                <span>{bytes(v.sizeBytes + m.companions.reduce((n, f) => n + f.size, 0))} to download with its text files</span>
                <span>{m.steps} steps</span>
                <span>{m.license}{m.commercialOk || /selling/.test(m.license) ? "" : " · check before selling pictures"}</span>
                {m.community && <span>Community model by {m.source.split("/")[0]}</span>}
                {m.mature && <span>Adult content possible</span>}
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
}

function DownloadRow({ m, vi, state, onDownload, onPause, onDelete }: { m: ImageModel; vi: number; state?: DlState; onDownload: () => void; onPause: () => void; onDelete: () => void }) {
  const v = m.variants[vi];
  const total = v.sizeBytes + m.companions.reduce((n, f) => n + f.size, 0);
  const busy = state && (state.phase === "downloading" || state.phase === "verifying");
  if (v.installed && !busy) {
    return (
      <div className="row" style={{ gap: 8 }}>
        <span className="pill ok">Downloaded</span>
        <button className="btn sm ghost" onClick={onDelete} title="Deletes this version's file. Text files shared with other models stay.">Delete this version</button>
      </div>
    );
  }
  if (busy) {
    const pct = state.total > 0 ? Math.min(100, Math.round((state.bytes / state.total) * 100)) : 0;
    return (
      <div className="row" style={{ gap: 8, alignItems: "center" }}>
        <progress value={pct} max={100} style={{ flex: 1 }} aria-label="Download progress" />
        <span className="faint small">{state.phase === "verifying" ? "Checking the file…" : `${pct}% · ${bytes(state.bytes)} of ${bytes(state.total)}`}</span>
        {state.phase === "downloading" && <button className="btn sm ghost" onClick={onPause}>Pause</button>}
      </div>
    );
  }
  return (
    <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
      <button className="btn sm" onClick={onDownload}>{state?.phase === "paused" ? "Resume" : "Download"} {v.quant} ({bytes(total)})</button>
      {state?.phase === "failed" && <span className="faint small" role="alert">{state.message}</span>}
    </div>
  );
}
