import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorText } from "../../lib/api";
import { fitPoem, fontFor, POSTER_STYLES, quietestSide, regionFor, wantsLightText, type Side } from "../../lib/posterLayout";
import type { ImageGalleryItem, ImageMade, ImageModel } from "../../lib/types";
import { PicturePicker } from "./Gallery";

const SAMPLE = `The lighthouse keeps its lamp alight\nthough no one climbs the stair,\nand every ship that passes by\nis someone's answered prayer.`;

/** Poem and quote posters: art behind the words, with the words set where the picture is quiet. */
export function Poster({ models }: { models: ImageModel[] }) {
  const [poem, setPoem] = useState(SAMPLE);
  const [title, setTitle] = useState("The Lighthouse");
  const [by, setBy] = useState("");
  const [style, setStyle] = useState("typewriter");
  const [sideChoice, setSideChoice] = useState<"auto" | Side>("auto");
  const [source, setSource] = useState<{ path: string; url: string; w: number; h: number; grid: number[] } | null>(null);
  const [picking, setPicking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<ImageGalleryItem | null>(null);
  const [scale, setScale] = useState(100);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [img, setImg] = useState<HTMLImageElement | null>(null);

  const load = useCallback(async (path: string) => {
    setError(null);
    try {
      const shown = await api.imagePreview(path, 2400);
      const grid = await api.imageBrightness(path, 16, 12);
      setSource({ path, url: shown.preview, w: shown.width, h: shown.height, grid });
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    if (!source) return setImg(null);
    const i = new Image();
    i.onload = () => setImg(i);
    i.src = source.url;
  }, [source]);

  const side: Side = source ? (sideChoice === "auto" ? quietestSide(source.grid, 16, 12) : sideChoice) : "left";
  const light = source ? wantsLightText(source.grid, 16, 12, side) : true;

  useEffect(() => {
    const c = canvas.current;
    if (c && img && source) draw(c, img, { poem, title, by, style, side, light, scale });
  }, [img, source, poem, title, by, style, side, light, scale]);

  const makeArt = async () => {
    const gen = models.filter((m) => m.kind === "generate").flatMap((m) => m.variants.filter((v) => v.installed).map((v) => ({ m, v })));
    const pick = gen.sort((a, b) => b.m.quality - a.m.quality)[0];
    if (!pick) return setError("Download a picture model first (Z-Image Turbo is quick and good).");
    setBusy(true);
    setError(null);
    try {
      const idea = `${poem.replace(/\s+/g, " ").slice(0, 220)}`;
      const prompt = `A wide, atmospheric illustration for the poem "${title}": ${idea}. Painterly, moody light, a large calm dark area on the left side with nothing in it, no text, no lettering`;
      const made: ImageMade = await api.imageGenerate({ model: pick.m.id, quant: pick.v.quant, prompt, width: 1344, height: 768 });
      await load(made.path);
    } catch (e) {
      const t = errorText(e);
      if (t !== "cancelled") setError(t);
    }
    setBusy(false);
  };

  const exportPoster = async (): Promise<ImageGalleryItem | null> => {
    const c = document.createElement("canvas");
    if (!img) return null;
    draw(c, img, { poem, title, by, style, side, light, scale });
    return api.imagePosterSave(c.toDataURL("image/png"), title || "Poster");
  };

  return (
    <div className="section">
      <h4>Poem and quote posters</h4>
      <p className="faint">
        Put a poem on a picture. BYTE finds the quiet part of the picture, sets the words there, and keeps them readable. Pick art from your gallery, or let BYTE draw art for the poem.
      </p>
      <div className="row" style={{ gap: 8, flexWrap: "wrap", marginBottom: 8 }}>
        <button className="btn" onClick={() => setPicking(true)}>Pick art from the gallery</button>
        <button className="btn" disabled={busy} onClick={() => void makeArt()}>{busy ? "Drawing the art…" : "Draw art for this poem"}</button>
      </div>
      {picking && (
        <PicturePicker
          onClose={() => setPicking(false)}
          onPick={(p) => {
            setPicking(false);
            void load(p);
          }}
        />
      )}
      <label className="field" style={{ display: "block" }}>
        <span>The poem</span>
        <textarea className="text-input" style={{ width: "100%", minHeight: 120 }} value={poem} onChange={(e) => setPoem(e.target.value)} aria-label="The poem" />
      </label>
      <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
        <input className="text-input" placeholder="Title" value={title} onChange={(e) => setTitle(e.target.value)} aria-label="Title" />
        <input className="text-input" placeholder="By (optional)" value={by} onChange={(e) => setBy(e.target.value)} aria-label="By" />
      </div>
      <div className="chips" role="group" aria-label="Poster style" style={{ margin: "8px 0" }}>
        {POSTER_STYLES.map((s) => (
          <button key={s.id} className="chip" title={s.hint} aria-pressed={style === s.id} onClick={() => setStyle(s.id)}>
            {s.label}
          </button>
        ))}
      </div>
      <div className="row" style={{ gap: 12, flexWrap: "wrap", alignItems: "center" }}>
        <label className="field">
          <span>Where the words go</span>
          <select value={sideChoice} onChange={(e) => setSideChoice(e.target.value as "auto" | Side)} aria-label="Where the words go">
            <option value="auto">Where the picture is quiet</option>
            <option value="left">Left</option>
            <option value="right">Right</option>
            <option value="top">Top</option>
            <option value="bottom">Bottom</option>
          </select>
        </label>
        <label className="field">
          <span>Text size</span>
          <input type="range" min={60} max={110} value={scale} onChange={(e) => setScale(Number(e.target.value))} aria-label="Text size" />
        </label>
      </div>
      {error && <div className="banner danger" role="alert">{error}</div>}
      {source ? (
        <>
          <canvas ref={canvas} style={{ width: "100%", borderRadius: 8, marginTop: 8 }} aria-label="Poster preview" />
          <div className="row" style={{ gap: 8, marginTop: 8 }}>
            <button
              className="btn primary"
              onClick={() =>
                void exportPoster()
                  .then((it) => it && setSaved(it))
                  .catch((e) => setError(errorText(e)))
              }
            >
              Save to my gallery
            </button>
            {saved && (
              <button
                className="btn"
                onClick={() =>
                  void saveDialog({ defaultPath: "poster.png", filters: [{ name: "Picture", extensions: ["png"] }] })
                    .then((dest) => (dest ? api.imageCopyTo(saved.path, dest) : undefined))
                    .catch((e) => setError(errorText(e)))
                }
              >
                Save a copy…
              </button>
            )}
            {saved && <span className="faint small">Saved. {saved.width} × {saved.height}</span>}
          </div>
        </>
      ) : (
        <p className="faint">Pick or draw some art to see the poster.</p>
      )}
    </div>
  );
}

interface DrawOpts {
  poem: string;
  title: string;
  by: string;
  style: string;
  side: Side;
  light: boolean;
  scale: number;
}

/** Draws the art and the words. Works at the picture's own size (up to 2400 px on the long side). */
export function draw(c: HTMLCanvasElement, img: HTMLImageElement, o: DrawOpts) {
  const f = Math.min(1, 2400 / Math.max(img.naturalWidth, img.naturalHeight));
  const W = Math.round(img.naturalWidth * f);
  const H = Math.round(img.naturalHeight * f);
  c.width = W;
  c.height = H;
  const g = c.getContext("2d");
  if (!g) return;
  g.drawImage(img, 0, 0, W, H);
  const region = regionFor(o.side, W, H);
  // A soft shade behind the words keeps them readable on any picture.
  const shade = o.light ? "0,0,0" : "255,255,255";
  const grad =
    o.side === "left" ? g.createLinearGradient(0, 0, W * 0.55, 0) : o.side === "right" ? g.createLinearGradient(W, 0, W * 0.45, 0) : o.side === "top" ? g.createLinearGradient(0, 0, 0, H * 0.55) : g.createLinearGradient(0, H, 0, H * 0.45);
  grad.addColorStop(0, `rgba(${shade},0.62)`);
  grad.addColorStop(1, `rgba(${shade},0)`);
  g.fillStyle = grad;
  g.fillRect(0, 0, W, H);

  const upper = o.style === "neon" || o.style === "paint";
  const measure = (s: string, size: number) => {
    g.font = fontFor(o.style, size);
    return g.measureText(upper ? s.toUpperCase() : s).width;
  };
  const headerSize = Math.round(Math.min(W, H) * 0.05 * (o.scale / 100));
  const header = o.title ? headerSize * 2.2 : 0;
  const body: typeof region = { ...region, y: region.y + header, h: region.h - header - (o.by ? headerSize * 1.4 : 0) };
  const fit = fitPoem(o.poem, body, measure, Math.round(Math.min(W, H) * 0.06 * (o.scale / 100)), 14);
  const ink = o.style === "neon" ? "#ff4a4a" : o.light ? "#f1ead8" : "#1d1a16";
  g.textBaseline = "top";
  if (o.title) {
    g.font = fontFor(o.style, headerSize);
    paint(g, o.style, upper ? o.title.toUpperCase() : o.title, region.x, region.y, ink, headerSize);
  }
  if (!fit) {
    g.font = fontFor(o.style, 20);
    g.fillStyle = ink;
    g.fillText("The poem is too long for this picture. Try a larger picture or less text.", region.x, body.y);
    return;
  }
  const totalW = fit.columns.length * fit.colWidth + (fit.columns.length - 1) * fit.gap;
  const startX = o.side === "right" ? region.x + region.w - totalW : region.x;
  fit.columns.forEach((col, ci) => {
    const x = startX + ci * (fit.colWidth + fit.gap);
    col.forEach((line, li) => {
      if (!line) return;
      // The elegant style indents every second line of a stanza.
      const indent = o.style === "elegant" && li % 2 === 1 ? fit.size * 1.2 : 0;
      g.font = fontFor(o.style, fit.size);
      paint(g, o.style, upper ? line.toUpperCase() : line, x + indent, body.y + li * fit.lineHeight, ink, fit.size);
    });
  });
  if (o.by) {
    g.font = fontFor(o.style === "paint" ? "typewriter" : o.style, Math.round(fit.size * 0.8));
    g.fillStyle = ink;
    g.globalAlpha = 0.8;
    g.fillText(`— ${o.by}`, region.x, region.y + region.h - fit.size * 1.2);
    g.globalAlpha = 1;
  }
}

/** One line of text in a style: neon glows, paint drips, the others are plain ink with a faint shadow. */
function paint(g: CanvasRenderingContext2D, style: string, text: string, x: number, y: number, ink: string, size: number) {
  if (style === "neon") {
    g.shadowColor = "#ff2020";
    g.shadowBlur = size * 0.7;
    g.fillStyle = ink;
    g.fillText(text, x, y);
    g.shadowBlur = size * 0.2;
    g.fillStyle = "#ffe0e0";
    g.fillText(text, x, y);
    g.shadowBlur = 0;
    return;
  }
  g.shadowColor = "rgba(0,0,0,0.35)";
  g.shadowBlur = size * 0.12;
  g.fillStyle = ink;
  g.fillText(text, x, y);
  g.shadowBlur = 0;
  if (style === "paint") {
    // Drips under some letters, the same every time for the same text.
    let seed = text.length * 97 + Math.round(x);
    const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 4294967296);
    const w = g.measureText(text).width;
    for (let dx = size * 0.3; dx < w; dx += size * (0.5 + rnd() * 0.9)) {
      if (rnd() < 0.5) continue;
      const len = size * (0.15 + rnd() * 0.55);
      g.fillRect(x + dx, y + size * 0.85, Math.max(2, size * 0.07), len);
      g.beginPath();
      g.arc(x + dx + Math.max(2, size * 0.07) / 2, y + size * 0.85 + len, Math.max(2, size * 0.07), 0, Math.PI * 2);
      g.fill();
    }
  }
}
