import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";

import { api, errorText } from "../../lib/api";
import { took } from "../../lib/imageSizes";
import type { ImageEditRequest, ImageMade, ImageModel, ImageShown } from "../../lib/types";
import { useStore } from "../../state/store";
import { forTask } from "../models/ImageCatalog";
import { PicturePicker } from "./Gallery";
import { BeforeAfter, PHASE, useImageProgress } from "./shared";

/** One-click starting points for "change with words". */
export const EDIT_PRESETS: { id: string; label: string; prompt: string; ask?: string }[] = [
  { id: "remove", label: "Remove a person or thing", ask: "What should go?", prompt: "Remove {x} from the photo. Fill in the background naturally and keep everything else exactly the same." },
  { id: "background", label: "Remove the background", prompt: "Remove the background and replace it with plain white. Keep the subject exactly the same." },
  { id: "restore", label: "Restore an old photo", prompt: "Restore this old photo: remove scratches, dust and stains, sharpen the faces, and fix the faded contrast. Keep the people looking exactly the same." },
  { id: "colorize", label: "Colorize", prompt: "Colorize this black and white photo with natural, period-correct colours. Keep everything else the same." },
  { id: "light", label: "Change the lighting", ask: "Which lighting?", prompt: "Change the lighting to {x}. Keep the people and the scene exactly the same." },
  { id: "style", label: "Change the style", ask: "Which style?", prompt: "Turn this photo into {x}. Keep the composition and the people recognisable." },
  { id: "words", label: "Change the words on a sign", ask: "Change what to what?", prompt: "Change the text in the picture: {x}. Keep everything else the same." },
  { id: "free", label: "Something else", ask: "What should change?", prompt: "{x}" },
];

const ASK_HINT: Record<string, string> = {
  remove: "the man on the left",
  light: "warm golden-hour sunlight",
  style: "a watercolor painting",
  words: "'OPEN' to 'CLOSED'",
  free: "make the sky stormy",
};

export function presetPrompt(id: string, x: string): string {
  const p = EDIT_PRESETS.find((e) => e.id === id);
  return p ? p.prompt.replace("{x}", x.trim()) : x;
}

type Tab = "words" | "bigger" | "crop" | "blur" | "grain" | "finish";
const TABS: { id: Tab; label: string }[] = [
  { id: "words", label: "Change with words" },
  { id: "bigger", label: "Make bigger" },
  { id: "crop", label: "Crop" },
  { id: "blur", label: "Motion blur" },
  { id: "grain", label: "Film grain" },
  { id: "finish", label: "Camera finish" },
];

const RATIOS: { id: string; label: string; r: number }[] = [
  { id: "sq", label: "Square", r: 1 },
  { id: "45", label: "4:5", r: 4 / 5 },
  { id: "32", label: "3:2", r: 3 / 2 },
  { id: "169", label: "16:9", r: 16 / 9 },
  { id: "916", label: "9:16", r: 9 / 16 },
];

/** The biggest rectangle of a ratio that fits the picture, moved by `px`/`py` (0-1) and shrunk by `zoom` (1 = biggest). */
export function cropRect(w: number, h: number, ratio: number, zoom: number, px: number, py: number) {
  let cw = w;
  let ch = w / ratio;
  if (ch > h) {
    ch = h;
    cw = h * ratio;
  }
  cw /= Math.max(1, zoom);
  ch /= Math.max(1, zoom);
  return { x: ((w - cw) * px) / w, y: ((h - ch) * py) / h, w: cw / w, h: ch / h };
}

/** Edit a photo: with a sentence, or with plain tools (enlarge, crop, blur, grain, camera finish). */
export function EditPhoto({ models, start }: { models: ImageModel[]; start?: string }) {
  const [source, setSource] = useState<ImageShown | null>(null);
  const [result, setResult] = useState<(ImageMade & { before: string }) | null>(null);
  const [picking, setPicking] = useState(false);
  const [tab, setTab] = useState<Tab>("words");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const progress = useImageProgress();
  const settings = useStore((s) => s.settings);

  const load = async (path: string) => {
    setError(null);
    setResult(null);
    try {
      setSource(await api.imagePreview(path, 1400));
    } catch (e) {
      setError(errorText(e));
    }
  };
  useEffect(() => {
    if (start) void load(start);
  }, [start]);

  const editors = useMemo(() => models.filter((m) => forTask(m, "edit")).flatMap((m) => m.variants.filter((v) => v.installed).map((v) => ({ key: `${m.id}|${v.quant}`, m, v }))), [models]);
  const upscalers = models.filter((m) => m.kind === "upscale" && m.variants[0]?.installed);

  const run = async (req: Omit<ImageEditRequest, "source">) => {
    if (!source) return;
    setBusy(true);
    setError(null);
    try {
      const made = await api.imageEdit({ ...req, source: source.path });
      setResult({ ...made, before: source.preview });
    } catch (e) {
      const t = errorText(e);
      if (t !== "cancelled") setError(t);
    }
    setBusy(false);
  };
  const keepEditing = () => {
    if (result) void load(result.path);
  };

  return (
    <div className="section">
      <h4>Edit a photo</h4>
      <p className="faint">Change a picture you made or a photo of your own. Your photos stay on this PC.</p>
      <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
        <button className="btn" onClick={() => setPicking(true)}>Pick from my gallery</button>
        <button
          className="btn"
          onClick={() =>
            void openDialog({ multiple: false, filters: [{ name: "Pictures", extensions: ["png", "jpg", "jpeg"] }] })
              .then((p) => (typeof p === "string" ? load(p) : undefined))
              .catch((e) => setError(errorText(e)))
          }
        >
          Choose a photo…
        </button>
      </div>
      {picking && <PicturePicker onClose={() => setPicking(false)} onPick={(p) => (setPicking(false), void load(p))} />}
      {error && <div className="banner danger" role="alert">{error}</div>}
      {!settings?.imageEdit && <p className="faint">Turn on “Edit photos” at the top of this page first.</p>}
      {source && settings?.imageEdit && (
        <>
          <div className="chips" role="tablist" aria-label="Edit" style={{ margin: "10px 0" }}>
            {TABS.map((t) => (
              <button key={t.id} className="chip" role="tab" aria-selected={tab === t.id} aria-pressed={tab === t.id} onClick={() => setTab(t.id)}>
                {t.label}
              </button>
            ))}
          </div>
          {tab === "words" && <Words editors={editors} busy={busy} run={run} />}
          {tab === "bigger" && <Bigger upscalers={upscalers} busy={busy} run={run} />}
          {tab === "crop" && <Crop source={source} busy={busy} run={run} />}
          {tab === "blur" && <Blur busy={busy} run={run} />}
          {tab === "grain" && <Strength label="Film grain" op="grain" start={40} busy={busy} run={run} />}
          {tab === "finish" && <Strength label="Camera finish" op="finish" start={settings.imageFinishStrength ?? 40} busy={busy} run={run} />}
          <div className="row" style={{ gap: 8, alignItems: "center", margin: "8px 0" }}>
            {busy && <button className="btn" onClick={() => void api.imageCancel()}>Stop</button>}
            {busy && progress && (
              <>
                {progress.total > 0 ? <progress value={progress.step} max={progress.total} style={{ flex: 1 }} aria-label="Progress" /> : null}
                <span className="faint small" aria-live="polite">{PHASE[progress.phase]}{progress.total > 0 ? ` ${progress.step}/${progress.total}` : ""}</span>
              </>
            )}
          </div>
          {result ? (
            <>
              <BeforeAfter before={result.before} after={result.preview} />
              <p className="faint small">
                {result.width} × {result.height} · {took(result.seconds)} · {result.notes.join(" · ")}
              </p>
              <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                <button className="btn" onClick={keepEditing}>Keep editing this</button>
                <button
                  className="btn"
                  onClick={() =>
                    void saveDialog({ defaultPath: result.path.split(/[\\/]/).pop(), filters: [{ name: "Picture", extensions: ["png", "jpg"] }] })
                      .then((dest) => (dest ? api.imageCopyTo(result.path, dest) : undefined))
                      .catch((e) => setError(errorText(e)))
                  }
                >
                  Save a copy…
                </button>
              </div>
            </>
          ) : (
            <img src={source.preview} alt="Your picture" style={{ width: "100%", borderRadius: 8 }} />
          )}
        </>
      )}
    </div>
  );
}

type Runner = (req: Omit<ImageEditRequest, "source">) => Promise<void>;

function Words({ editors, busy, run }: { editors: { key: string; m: ImageModel; v: ImageModel["variants"][number] }[]; busy: boolean; run: Runner }) {
  const [pick, setPick] = useState("");
  const [preset, setPreset] = useState("remove");
  const [x, setX] = useState("");
  const [size, setSize] = useState(1024);
  useEffect(() => {
    if (!pick && editors[0]) setPick(editors[0].key);
  }, [editors, pick]);
  if (editors.length === 0) {
    return <p className="faint">Editing with words needs an editing model. Download one under Image models → Edit a photo. Qwen-Image Edit is the strongest; FLUX.2 klein 4B is small and quick.</p>;
  }
  const p = EDIT_PRESETS.find((e) => e.id === preset)!;
  const chosen = editors.find((e) => e.key === pick)!;
  const ready = !p.ask || x.trim();
  return (
    <>
      <div className="chips" role="group" aria-label="What to do" style={{ marginBottom: 8 }}>
        {EDIT_PRESETS.map((e) => (
          <button key={e.id} className="chip" aria-pressed={preset === e.id} onClick={() => (setPreset(e.id), setX(""))}>
            {e.label}
          </button>
        ))}
      </div>
      {p.ask && (
        <label className="field" style={{ display: "block" }}>
          <span>{p.ask}</span>
          <input className="text-input" style={{ width: "100%" }} placeholder={`for example: ${ASK_HINT[preset] ?? ""}`} value={x} onChange={(e) => setX(e.target.value)} aria-label={p.ask} />
        </label>
      )}
      <label className="field">
        <span>Model</span>
        <select value={pick} onChange={(e) => setPick(e.target.value)} aria-label="Editing model">
          {editors.map((e) => (
            <option key={e.key} value={e.key}>{e.m.name} · {e.v.quant}</option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>Size<small>Bigger keeps more detail and takes longer.</small></span>
        <select value={size} onChange={(e) => setSize(Number(e.target.value))} aria-label="Size">
          <option value={768}>Quick (768)</option>
          <option value={1024}>Normal (1024)</option>
          <option value={1344}>Large (1344)</option>
        </select>
      </label>
      <button className="btn primary" disabled={busy || !ready} onClick={() => void run({ op: "edit", model: chosen.m.id, quant: chosen.v.quant, prompt: presetPrompt(preset, x), maxSide: size })}>
        {busy ? "Working…" : "Make the change"}
      </button>
    </>
  );
}

function Bigger({ upscalers, busy, run }: { upscalers: ImageModel[]; busy: boolean; run: Runner }) {
  const [up, setUp] = useState("");
  const [scale, setScale] = useState(4);
  if (upscalers.length === 0) return <p className="faint">Download an upscaler under Image models → Upscale first. 4xNomosWebPhoto is the best for photos of people.</p>;
  const chosen = upscalers.find((u) => u.id === up);
  return (
    <>
      <p className="faint small">Fixes the blocky look of a small or zoomed photo and makes it bigger.</p>
      <label className="field">
        <span>Upscaler{chosen && <small>{chosen.tagline} {chosen.strengths}</small>}</span>
        <select value={up} onChange={(e) => setUp(e.target.value)} aria-label="Upscaler">
          <option value="">Best for photos (automatic)</option>
          {upscalers.map((m) => (
            <option key={m.id} value={m.id}>{m.name}</option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>How much bigger</span>
        <select value={scale} onChange={(e) => setScale(Number(e.target.value))} aria-label="How much bigger">
          <option value={4}>4 times (most detail)</option>
          <option value={2}>2 times (more natural, smaller file)</option>
        </select>
      </label>
      <button className="btn primary" disabled={busy} onClick={() => void run({ op: "enlarge", upscaler: up, scale })}>{busy ? "Working…" : "Make it bigger"}</button>
    </>
  );
}

function Crop({ source, busy, run }: { source: ImageShown; busy: boolean; run: Runner }) {
  const [ratio, setRatio] = useState("45");
  const [zoom, setZoom] = useState(1);
  const [px, setPx] = useState(0.5);
  const [py, setPy] = useState(0.5);
  const r = RATIOS.find((x) => x.id === ratio)!;
  const rect = cropRect(source.width, source.height, r.r, zoom, px, py);
  return (
    <>
      <div className="chips" role="group" aria-label="Shape" style={{ marginBottom: 8 }}>
        {RATIOS.map((x) => (
          <button key={x.id} className="chip" aria-pressed={ratio === x.id} onClick={() => setRatio(x.id)}>{x.label}</button>
        ))}
      </div>
      <div style={{ position: "relative", lineHeight: 0, marginBottom: 8 }}>
        <img src={source.preview} alt="" style={{ width: "100%", borderRadius: 8 }} />
        <div style={{ position: "absolute", left: `${rect.x * 100}%`, top: `${rect.y * 100}%`, width: `${rect.w * 100}%`, height: `${rect.h * 100}%`, outline: "2px solid #fff", boxShadow: "0 0 0 9999px rgba(0,0,0,0.55)", pointerEvents: "none" }} />
      </div>
      <label className="field"><span>Zoom in</span><input type="range" min={1} max={4} step={0.05} value={zoom} onChange={(e) => setZoom(Number(e.target.value))} aria-label="Zoom in" /></label>
      <label className="field"><span>Left ↔ right</span><input type="range" min={0} max={1} step={0.01} value={px} onChange={(e) => setPx(Number(e.target.value))} aria-label="Left to right" /></label>
      <label className="field"><span>Up ↔ down</span><input type="range" min={0} max={1} step={0.01} value={py} onChange={(e) => setPy(Number(e.target.value))} aria-label="Up to down" /></label>
      <button className="btn primary" disabled={busy} onClick={() => void run({ op: "crop", ...rect })}>Crop</button>
    </>
  );
}

function Blur({ busy, run }: { busy: boolean; run: Runner }) {
  const [length, setLength] = useState(24);
  const [angle, setAngle] = useState(0);
  return (
    <>
      <p className="faint small">Smears the picture in one direction, like a camera that moved while the shutter was open.</p>
      <label className="field"><span>How much</span><input type="range" min={4} max={80} value={length} onChange={(e) => setLength(Number(e.target.value))} aria-label="How much" /></label>
      <label className="field"><span>Direction ({angle}°)<small>0 is sideways, 90 is up and down.</small></span><input type="range" min={0} max={179} value={angle} onChange={(e) => setAngle(Number(e.target.value))} aria-label="Direction" /></label>
      <button className="btn primary" disabled={busy} onClick={() => void run({ op: "motion", length, angle })}>{busy ? "Working…" : "Add motion blur"}</button>
    </>
  );
}

function Strength({ label, op, start, busy, run }: { label: string; op: "grain" | "finish"; start: number; busy: boolean; run: Runner }) {
  const [k, setK] = useState(start);
  return (
    <>
      <p className="faint small">{op === "grain" ? "Fine film grain only." : "Slight softness, colour fringing, flatter colour, a darker corner and grain, so it looks like a camera took it."}</p>
      <label className="field"><span>{label} strength: {k}</span><input type="range" min={0} max={100} value={k} onChange={(e) => setK(Number(e.target.value))} aria-label={`${label} strength`} /></label>
      <button className="btn primary" disabled={busy} onClick={() => void run({ op, strength: k })}>{busy ? "Working…" : `Add ${label.toLowerCase()}`}</button>
    </>
  );
}
