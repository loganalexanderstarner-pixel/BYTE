import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { sizeOf, SIZES, took } from "../../lib/imageSizes";
import { bytes } from "../../lib/format";
import type { ImageEngine, ImageMade, ImageModel, ImageProgress } from "../../lib/types";
import { useStore } from "../../state/store";
import { ImageCatalog, toDl, type DlState } from "../models/ImageCatalog";

/** The Images tab: switches, the Generate form, and the model catalog (Windows). */
export function ImageStudio() {
  const settings = useStore((s) => s.settings);
  const [models, setModels] = useState<ImageModel[] | null>(null);
  const reload = useCallback(() => {
    api.imageCatalog().then(setModels).catch(() => setModels([]));
  }, []);
  useEffect(reload, [reload]);
  const [engine, setEngine] = useState<ImageEngine | null>(null);
  const reloadEngine = useCallback(() => {
    api.imageEngine().then(setEngine).catch(() => setEngine(null));
  }, []);
  useEffect(reloadEngine, [reloadEngine]);
  return (
    <>
      <h3>Images</h3>
      <ImageSwitches models={models} />
      {settings?.imageStudio && <EngineRow engine={engine} reload={reloadEngine} />}
      {settings?.imageStudio && engine?.active && <Generate models={models ?? []} />}
      <h4 style={{ marginTop: 24 }}>Image models</h4>
      <ImageCatalog models={models} reload={reload} />
    </>
  );
}

/** The picture engine: shown until one is installed, then as a single line. */
function EngineRow({ engine, reload }: { engine: ImageEngine | null; reload: () => void }) {
  const [dl, setDl] = useState<Record<string, DlState>>({});
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let off: (() => void) | undefined;
    void api.onImageDownload((e) => {
      if (!e.id.startsWith("engine:")) return;
      setDl((d) => ({ ...d, [e.id]: toDl(e, d[e.id]) }));
      if (e.kind === "finished") reload();
    }).then((u) => (off = u));
    return () => off?.();
  }, [reload]);
  if (!engine) return null;
  if (engine.active) {
    const o = engine.options.find((x) => x.id === engine.active);
    return <p className="faint small">Picture engine: {o?.label ?? "custom"}. It makes the pictures on this PC.</p>;
  }
  return (
    <div className="section">
      <h4>Picture engine</h4>
      <p className="faint">
        Pictures are drawn by a small program that BYTE downloads once (checked against a fixed checksum). Pick the one that matches your graphics card. The first is the one BYTE recommends for this PC.
      </p>
      {[...engine.options].sort((a, b) => Number(b.id === engine.recommended) - Number(a.id === engine.recommended)).map((o) => {
        const st = dl[`engine:${o.id}`];
        const busy = st && (st.phase === "downloading" || st.phase === "verifying");
        const pct = st && st.total > 0 ? Math.min(100, Math.round((st.bytes / st.total) * 100)) : 0;
        return (
          <div key={o.id} className="model-card" style={{ marginBottom: 8 }}>
            <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
              <b className="grow">{o.label}</b>
              {o.id === engine.recommended && <span className="pill ok">Recommended</span>}
            </div>
            <div className="faint small">{o.note}</div>
            {busy ? (
              <div className="row" style={{ gap: 8, alignItems: "center" }}>
                <progress value={pct} max={100} style={{ flex: 1 }} aria-label="Download progress" />
                <span className="faint small">{st.phase === "verifying" ? "Unpacking…" : `${pct}%`}</span>
              </div>
            ) : (
              <div className="row" style={{ gap: 8 }}>
                <button
                  className="btn sm"
                  onClick={() => {
                    setError(null);
                    api.imageEngineInstall(o.id).catch((e) => setError(errorText(e)));
                  }}
                >
                  Download ({bytes(o.sizeBytes)})
                </button>
                {st?.phase === "failed" && <span className="faint small" role="alert">{st.message}</span>}
              </div>
            )}
          </div>
        );
      })}
      {error && <div className="banner danger" role="alert">{error}</div>}
    </div>
  );
}

function ImageSwitches({ models }: { models: ImageModel[] | null }) {
  const s = useStore((st) => st.settings);
  const update = useStore((st) => st.updateSettings);
  if (!s) return null;
  const upscalers = (models ?? []).filter((m) => m.kind === "upscale" && m.variants[0]?.installed);
  return (
    <div className="section">
      <label className="field">
        <span>
          Make pictures
          <small>Turns on the Generate form below. Pictures are made on this PC; your prompts and photos never leave it. Off in kids mode.</small>
        </span>
        <input type="checkbox" checked={s.imageStudio ?? false} onChange={(e) => void update({ imageStudio: e.target.checked })} />
      </label>
      {s.imageStudio && (
        <>
          <label className="field">
            <span>
              Realistic people
              <small>
                Lets you ask for pictures that look like real people. Such pictures can be mistaken for photographs. Don't make pictures of real, named people, and don't use them to mislead anyone. What you make and share is your responsibility.
              </small>
            </span>
            <input type="checkbox" checked={s.imageRealPeople ?? false} onChange={(e) => void update({ imageRealPeople: e.target.checked })} />
          </label>
          <label className="field">
            <span>
              Detail pass
              <small>Draws a second, larger pass that adds fine detail (skin, eyes, fabric). Takes about twice as long.</small>
            </span>
            <input type="checkbox" checked={s.imageHires ?? false} onChange={(e) => void update({ imageHires: e.target.checked })} />
          </label>
          <label className="field">
            <span>
              Enlarge each picture
              <small>Makes the finished picture 2 or 4 times bigger with an upscaler. Download one under Image models → Upscale.</small>
            </span>
            <input type="checkbox" checked={s.imageUpscale ?? false} onChange={(e) => void update({ imageUpscale: e.target.checked })} />
          </label>
          {s.imageUpscale && (
            <label className="field">
              <span>
                Upscaler
                <small>Auto uses 4xNomosWebPhoto when it is downloaded, which keeps real skin texture.</small>
              </span>
              <select value={s.imageUpscaler ?? ""} onChange={(e) => void update({ imageUpscaler: e.target.value })} aria-label="Upscaler">
                <option value="">Auto (best installed)</option>
                {upscalers.map((m) => (
                  <option key={m.id} value={m.id}>{m.name}</option>
                ))}
              </select>
            </label>
          )}
          <label className="field">
            <span>
              Camera finish
              <small>Adds what a camera adds: slight softness, colour fringing, flatter colour, a darker corner and fine grain. Makes pictures look less computer-made.</small>
            </span>
            <input type="checkbox" checked={s.imagePhotoFinish ?? false} onChange={(e) => void update({ imagePhotoFinish: e.target.checked })} />
          </label>
          {s.imagePhotoFinish && (
            <label className="field">
              <span>
                Finish strength: {s.imageFinishStrength ?? 40}
                <small>Low is subtle. High is heavy film grain.</small>
              </span>
              <input type="range" min={0} max={100} value={s.imageFinishStrength ?? 40} onChange={(e) => void update({ imageFinishStrength: Number(e.target.value) })} aria-label="Finish strength" />
            </label>
          )}
          <label className="field">
            <span>
              Where the model lives
              <small>Auto keeps it in video memory while it fits and spills the rest into RAM. Video memory only is fastest. RAM only leaves your graphics card free for games.</small>
            </span>
            <select value={s.imagePlacement ?? "auto"} onChange={(e) => void update({ imagePlacement: e.target.value as "auto" | "vram" | "ram" })} aria-label="Where the model lives">
              <option value="auto">Auto</option>
              <option value="vram">Video memory only</option>
              <option value="ram">RAM and processor only</option>
            </select>
          </label>
          <label className="field">
            <span>
              Keep my pictures
              <small>What the picture folder keeps after each one is made.</small>
            </span>
            <select value={s.imageKeep ?? "all"} onChange={(e) => void update({ imageKeep: e.target.value as "all" | "saved" | "none" })} aria-label="Keep my pictures">
              <option value="all">Everything</option>
              <option value="saved">Only pictures I save</option>
              <option value="none">Nothing</option>
            </select>
          </label>
          <label className="field">
            <span>
              Remove settings from picture files
              <small>Pictures are saved without the prompt and settings the engine normally stores inside them. Good before you share a file.</small>
            </span>
            <input type="checkbox" checked={s.imageStripInfo ?? false} onChange={(e) => void update({ imageStripInfo: e.target.checked })} />
          </label>
        </>
      )}
    </div>
  );
}

const PHASE: Record<ImageProgress["phase"], string> = {
  loading: "Loading the model…",
  drawing: "Drawing",
  finishing: "Finishing the picture…",
  enlarging: "Enlarging…",
  polishing: "Adding the camera finish…",
};

function Generate({ models }: { models: ImageModel[] }) {
  const ready = models
    .filter((m) => m.kind === "generate")
    .flatMap((m) => m.variants.filter((v) => v.installed).map((v) => ({ key: `${m.id}|${v.quant}`, label: `${m.name} · ${v.quant}`, m, v })));
  const [pick, setPick] = useState("");
  const [prompt, setPrompt] = useState("A lighthouse on a rocky coast at golden hour, 35mm film photograph, soft haze over the water");
  const [negative, setNegative] = useState("");
  const [size, setSize] = useState("portrait");
  const [seed, setSeed] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<ImageProgress | null>(null);
  const [made, setMade] = useState<ImageMade | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!pick && ready[0]) setPick(ready[0].key);
  }, [ready, pick]);
  useEffect(() => {
    let off: (() => void) | undefined;
    void api.onImageProgress(setProgress).then((u) => (off = u));
    return () => off?.();
  }, []);

  const go = async () => {
    const chosen = ready.find((r) => r.key === pick);
    if (!chosen) return;
    setBusy(true);
    setError(null);
    setProgress({ phase: "loading", step: 0, total: 0 });
    const { w, h } = sizeOf(size);
    try {
      setMade(await api.imageGenerate({ model: chosen.m.id, quant: chosen.v.quant, prompt, negative, width: w, height: h, seed: seed.trim() ? Number(seed) : undefined }));
    } catch (e) {
      const t = errorText(e);
      if (t !== "cancelled") setError(t);
    }
    setBusy(false);
    setProgress(null);
  };

  return (
    <div className="section">
      <h4>Make a picture</h4>
      {ready.length === 0 ? (
        <p className="faint">Download an image model below first. A good start is Z-Image Turbo: quick, photo-real, and happy on small graphics cards.</p>
      ) : (
        <>
          <label className="field">
            <span>Model</span>
            <select value={pick} onChange={(e) => setPick(e.target.value)} aria-label="Model">
              {ready.map((r) => (
                <option key={r.key} value={r.key}>{r.label}</option>
              ))}
            </select>
          </label>
          <label className="field" style={{ display: "block" }}>
            <span>Describe the picture</span>
            <textarea className="text-input" style={{ width: "100%", minHeight: 90 }} value={prompt} onChange={(e) => setPrompt(e.target.value)} aria-label="Describe the picture" />
          </label>
          <details>
            <summary className="faint small">More options</summary>
            <label className="field">
              <span>What to avoid<small>Words for what you don't want, such as “blurry, extra fingers”. Quick models ignore this.</small></span>
              <input className="text-input" value={negative} onChange={(e) => setNegative(e.target.value)} aria-label="What to avoid" />
            </label>
            <label className="field">
              <span>Seed<small>The same seed and words make the same picture. Empty picks a new one each time.</small></span>
              <input className="text-input" value={seed} inputMode="numeric" onChange={(e) => setSeed(e.target.value.replace(/\D/g, ""))} aria-label="Seed" />
            </label>
          </details>
          <label className="field">
            <span>Shape</span>
            <select value={size} onChange={(e) => setSize(e.target.value)} aria-label="Shape">
              {SIZES.map((s) => (
                <option key={s.id} value={s.id}>{s.label}</option>
              ))}
            </select>
          </label>
          <div className="row" style={{ gap: 8, alignItems: "center" }}>
            {busy ? (
              <button className="btn" onClick={() => void api.imageCancel()}>Stop</button>
            ) : (
              <button className="btn primary" disabled={!prompt.trim()} onClick={() => void go()}>Make picture</button>
            )}
            {busy && progress && (
              <>
                {progress.total > 0 ? <progress value={progress.step} max={progress.total} style={{ flex: 1 }} aria-label="Progress" /> : null}
                <span className="faint small" aria-live="polite">{PHASE[progress.phase]}{progress.total > 0 ? ` ${progress.step}/${progress.total}` : ""}</span>
              </>
            )}
          </div>
          {error && <div className="banner danger" role="alert">{error}</div>}
          {made && (
            <figure style={{ margin: "16px 0 0" }}>
              <img src={made.preview} alt={prompt} style={{ maxWidth: "100%", borderRadius: 8 }} />
              <figcaption className="faint small">
                {made.width} × {made.height} · {took(made.seconds)} · seed {made.seed}{made.notes.length ? ` · ${made.notes.join(" · ")}` : ""}
                <br />
                Saved as {made.path}
              </figcaption>
            </figure>
          )}
        </>
      )}
    </div>
  );
}
