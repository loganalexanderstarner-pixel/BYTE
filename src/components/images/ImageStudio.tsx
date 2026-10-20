import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { sizeOf, SIZES, took } from "../../lib/imageSizes";
import { bytes } from "../../lib/format";
import type { ImageEngine, ImageMade, ImageModel } from "../../lib/types";
import { useStore } from "../../state/store";
import { ImageCatalog, toDl, type DlState } from "../models/ImageCatalog";
import { EditPhoto } from "./EditPhoto";
import { Gallery } from "./Gallery";
import { Poster } from "./Poster";
import { PHASE, useImageProgress } from "./shared";

type Section = "make" | "edit" | "poster" | "gallery" | "models";

/** The Images tab: switches, then Make / Edit / Poster / Gallery / Models, and the model catalog (Windows). */
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
  const [section, setSection] = useState<Section>("make");
  const [editStart, setEditStart] = useState<string | undefined>();
  const on = !!settings?.imageStudio;
  const ready = on && !!engine?.active;
  const sections: { id: Section; label: string }[] = [
    { id: "make", label: "Make" },
    ...(settings?.imageEdit ? [{ id: "edit" as const, label: "Edit" }] : []),
    ...(settings?.imagePoster ? [{ id: "poster" as const, label: "Posters" }] : []),
    ...(settings?.imageGallery ? [{ id: "gallery" as const, label: "Gallery" }] : []),
    { id: "models", label: "Models" },
  ];
  const current = sections.some((x) => x.id === section) ? section : "make";
  return (
    <>
      <h3>Images</h3>
      <ImageSwitches models={models} />
      {on && <EngineRow engine={engine} reload={reloadEngine} />}
      {ready ? (
        <>
          <div className="segmented" role="tablist" aria-label="Image tools" style={{ margin: "12px 0" }}>
            {sections.map((x) => (
              <button key={x.id} role="tab" aria-selected={current === x.id} aria-pressed={current === x.id} onClick={() => setSection(x.id)}>
                {x.label}
              </button>
            ))}
          </div>
          {current === "make" && <Generate models={models ?? []} />}
          {current === "edit" && <EditPhoto models={models ?? []} start={editStart} />}
          {current === "poster" && <Poster models={models ?? []} />}
          {current === "gallery" && (
            <Gallery
              onEdit={
                settings?.imageEdit
                  ? (p) => {
                      setEditStart(p);
                      setSection("edit");
                    }
                  : undefined
              }
            />
          )}
          {current === "models" && <ModelsList models={models} reload={reload} />}
        </>
      ) : (
        <ModelsList models={models} reload={reload} />
      )}
    </>
  );
}

function ModelsList({ models, reload }: { models: ImageModel[] | null; reload: () => void }) {
  return (
    <>
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
              Prompt helper
              <small>Adds a button that turns a short idea into a fuller description. Uses BYTE's chat model when it is running; otherwise a simple template. Nothing leaves this PC.</small>
            </span>
            <input type="checkbox" checked={s.imagePromptHelper ?? false} onChange={(e) => void update({ imagePromptHelper: e.target.checked })} />
          </label>
          <label className="field">
            <span>
              Compare models
              <small>Make the same picture with several models, side by side with the same seed, to see which you like. Each model is loaded in turn.</small>
            </span>
            <input type="checkbox" checked={s.imageCompare ?? false} onChange={(e) => void update({ imageCompare: e.target.checked })} />
          </label>
          <label className="field">
            <span>
              Gallery
              <small>Shows the pictures you made or edited, with what you asked for. Pictures stay in BYTE's folder on this PC.</small>
            </span>
            <input type="checkbox" checked={s.imageGallery ?? false} onChange={(e) => void update({ imageGallery: e.target.checked })} />
          </label>
          <label className="field">
            <span>
              Edit photos
              <small>Change a photo with a sentence (remove a person, restore an old photo, change the style), make it bigger, crop it, add film grain or motion blur. Needs an editing model for the sentence part.</small>
            </span>
            <input type="checkbox" checked={s.imageEdit ?? false} onChange={(e) => void update({ imageEdit: e.target.checked })} />
          </label>
          <label className="field">
            <span>
              Poem and quote posters
              <small>Put a poem or quote on a picture, with the words set where the picture is quiet. Several lettering styles.</small>
            </span>
            <input type="checkbox" checked={s.imagePoster ?? false} onChange={(e) => void update({ imagePoster: e.target.checked })} />
          </label>
          <label className="field">
            <span>
              Draw from chat and Hey BYTE
              <small>“Draw me a lighthouse” in a chat, or out loud, makes a picture with your best downloaded model. Never in private chats or kids mode.</small>
            </span>
            <input type="checkbox" checked={s.imageFromChat ?? false} onChange={(e) => void update({ imageFromChat: e.target.checked })} />
          </label>
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
                <small>
                  {upscalers.find((m) => m.id === s.imageUpscaler)
                    ? `${upscalers.find((m) => m.id === s.imageUpscaler)!.tagline} ${upscalers.find((m) => m.id === s.imageUpscaler)!.strengths}`
                    : "Auto uses 4xNomosWebPhoto when it is downloaded, which keeps real skin texture; otherwise the best photo upscaler you have."}
                </small>
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

function Generate({ models }: { models: ImageModel[] }) {
  const settings = useStore((s) => s.settings);
  const ready = models
    .filter((m) => m.kind === "generate")
    .flatMap((m) => m.variants.filter((v) => v.installed).map((v) => ({ key: `${m.id}|${v.quant}`, label: `${m.name} · ${v.quant}`, m, v })));
  const [pick, setPick] = useState("");
  const [also, setAlso] = useState<string[]>([]);
  const [prompt, setPrompt] = useState("A lighthouse on a rocky coast at golden hour, 35mm film photograph, soft haze over the water");
  const [negative, setNegative] = useState("");
  const [size, setSize] = useState("portrait");
  const [seed, setSeed] = useState("");
  const [busy, setBusy] = useState(false);
  const [helping, setHelping] = useState(false);
  const [helpNote, setHelpNote] = useState<string | null>(null);
  const [made, setMade] = useState<{ label: string; made: ImageMade }[]>([]);
  const [step, setStep] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const progress = useImageProgress();
  useEffect(() => {
    if (!pick && ready[0]) setPick(ready[0].key);
  }, [ready, pick]);

  const go = async () => {
    const first = ready.find((r) => r.key === pick);
    if (!first) return;
    const list = [first, ...(settings?.imageCompare ? also.map((k) => ready.find((r) => r.key === k)).filter((r): r is NonNullable<typeof r> => !!r && r.key !== first.key) : [])];
    // The same seed for every model, so the difference you see is the model.
    const useSeed = seed.trim() ? Number(seed) : list.length > 1 ? Math.floor(Math.random() * 2_000_000_000) : undefined;
    setBusy(true);
    setError(null);
    setMade([]);
    setSavedPath(null);
    const { w, h } = sizeOf(size);
    try {
      for (const [i, m] of list.entries()) {
        setStep(list.length > 1 ? `Picture ${i + 1} of ${list.length}: ${m.m.name}. ` : "");
        const r = await api.imageGenerate({ model: m.m.id, quant: m.v.quant, prompt, negative, width: w, height: h, seed: useSeed });
        setMade((cur) => [...cur, { label: m.label, made: r }]);
      }
    } catch (e) {
      const t = errorText(e);
      if (t !== "cancelled") setError(t);
    }
    setBusy(false);
    setStep("");
  };

  const help = async (style: string) => {
    setHelping(true);
    setHelpNote(null);
    setError(null);
    try {
      const r = await api.imageImprovePrompt(prompt, style);
      setPrompt(r.prompt);
      if (r.negative) setNegative(r.negative);
      setHelpNote(r.by === "model" ? "BYTE rewrote your idea. Edit it if you like." : "Added some standard details. Start BYTE's chat model for a fuller rewrite.");
    } catch (e) {
      setError(errorText(e));
    }
    setHelping(false);
  };

  return (
    <div className="section">
      <h4>Make a picture</h4>
      {ready.length === 0 ? (
        <p className="faint">Download an image model under “Models” first. A good start is Z-Image Turbo: quick, photo-real, and happy on small graphics cards.</p>
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
          {settings?.imageCompare && ready.length > 1 && (
            <fieldset className="field" style={{ border: 0, padding: 0 }}>
              <legend className="faint small">Also make it with (to compare)</legend>
              <div className="chips" role="group" aria-label="Compare with">
                {ready.filter((r) => r.key !== pick).map((r) => (
                  <button key={r.key} className="chip" aria-pressed={also.includes(r.key)} onClick={() => setAlso((a) => (a.includes(r.key) ? a.filter((k) => k !== r.key) : [...a, r.key]))}>
                    {r.label}
                  </button>
                ))}
              </div>
            </fieldset>
          )}
          <label className="field" style={{ display: "block" }}>
            <span>Describe the picture</span>
            <textarea className="text-input" style={{ width: "100%", minHeight: 90 }} value={prompt} onChange={(e) => setPrompt(e.target.value)} aria-label="Describe the picture" />
          </label>
          {settings?.imagePromptHelper && (
            <div className="row" style={{ gap: 6, flexWrap: "wrap", alignItems: "center", marginBottom: 6 }}>
              <span className="faint small">Improve my description as:</span>
              {[["photo", "A photo"], ["painting", "A painting"], ["poster", "A poster"], ["cartoon", "A cartoon"]].map(([id, label]) => (
                <button key={id} className="btn sm" disabled={helping || !prompt.trim()} onClick={() => void help(id)}>{label}</button>
              ))}
              {helping && <span className="faint small">Thinking…</span>}
            </div>
          )}
          {helpNote && <p className="faint small">{helpNote}</p>}
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
              <button className="btn primary" disabled={!prompt.trim()} onClick={() => void go()}>{settings?.imageCompare && also.length ? `Make ${also.length + 1} pictures` : "Make picture"}</button>
            )}
            {busy && progress && (
              <>
                {progress.total > 0 ? <progress value={progress.step} max={progress.total} style={{ flex: 1 }} aria-label="Progress" /> : null}
                <span className="faint small" aria-live="polite">{step}{PHASE[progress.phase]}{progress.total > 0 ? ` ${progress.step}/${progress.total}` : ""}</span>
              </>
            )}
          </div>
          {error && <div className="banner danger" role="alert">{error}</div>}
          {made.length > 0 && (
            <div style={{ display: "grid", gridTemplateColumns: made.length > 1 ? "repeat(auto-fit, minmax(260px, 1fr))" : "1fr", gap: 12, marginTop: 16 }}>
              {made.map((r) => (
                <figure key={r.made.path} style={{ margin: 0 }}>
                  <img src={r.made.preview} alt={prompt} style={{ maxWidth: "100%", borderRadius: 8 }} />
                  <figcaption className="faint small">
                    {made.length > 1 && <b>{r.label}<br /></b>}
                    {r.made.width} × {r.made.height} · {took(r.made.seconds)} · seed {r.made.seed}{r.made.notes.length ? ` · ${r.made.notes.join(" · ")}` : ""}
                    {settings?.imageKeep === "saved" && (
                      <>
                        <br />
                        <button className="btn sm" disabled={savedPath === r.made.path} onClick={() => void api.imageGallerySave(r.made.path, true).then(() => setSavedPath(r.made.path)).catch((e) => setError(errorText(e)))}>
                          {savedPath === r.made.path ? "Saved" : "Save this one"}
                        </button>
                      </>
                    )}
                  </figcaption>
                </figure>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
