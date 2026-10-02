import { Brain, Copy, Download, Loader2, NotebookPen, X, ZoomIn, ZoomOut } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { bounds, layout, toMarkdown, wrap } from "../../lib/mindmap";
import type { MapNode } from "../../lib/types";
import { useStore } from "../../state/store";

const LINE = 15;

/** A theme colour as an rgb() value (custom properties can hold color-mix(), which other apps can't read). */
function resolved(name: string): string {
  const el = document.createElement("span");
  el.style.color = `var(${name})`;
  document.body.appendChild(el);
  const c = getComputedStyle(el).color;
  el.remove();
  return c || "#888";
}

/** 🧠 A mind map of an answer or a note: click a branch to fold it; zoom, export, or save as a note. */
export function MindMapView() {
  const req = useStore((s) => s.mindmap);
  const close = useStore((s) => s.closeMindmap);
  const openNotes = useStore((s) => s.openNotes);
  const notesOn = useStore((s) => s.settings?.notesEnabled !== false);
  const [map, setMap] = useState<MapNode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [zoom, setZoom] = useState(1);
  const [copied, setCopied] = useState(false);
  const svg = useRef<SVGSVGElement>(null);

  useEffect(() => {
    if (!req) return;
    setMap(null);
    setError(null);
    if (!inTauri) return;
    void api.mindmapMake(req.text, req.title).then(setMap, (e) => setError(errorText(e)));
  }, [req]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

  const nodes = useMemo(() => (map ? layout(map, hidden) : []), [map, hidden]);
  const box = useMemo(() => (nodes.length ? bounds(nodes) : { x: -300, y: -200, w: 600, h: 400 }), [nodes]);
  const at = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  if (!req) return null;

  const toggle = (id: string) => {
    const next = new Set(hidden);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setHidden(next);
  };
  const svgText = () => {
    const el = svg.current;
    if (!el) return "";
    const clone = el.cloneNode(true) as SVGSVGElement;
    // Theme colours as real values, so the file looks the same outside BYTE.
    let s = new XMLSerializer().serializeToString(clone);
    for (const v of ["--accent", "--text", "--surface-2", "--bg", "--branch-0", "--branch-1", "--branch-2", "--branch-3", "--branch-4", "--branch-5", "--branch-6"]) {
      s = s.replaceAll(`var(${v})`, resolved(v));
    }
    return s;
  };
  const download = async (kind: "svg" | "png") => {
    const s = svgText();
    const name = `${(map?.label ?? "Mind map").replace(/[^\w -]/g, "")}.${kind}`;
    let blob: Blob;
    if (kind === "svg") blob = new Blob([s], { type: "image/svg+xml" });
    else {
      const img = new Image();
      img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(s)}`;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = box.w * 2;
      c.height = box.h * 2;
      const g = c.getContext("2d")!;
      g.fillStyle = resolved("--bg");
      g.fillRect(0, 0, c.width, c.height);
      g.drawImage(img, 0, 0, c.width, c.height);
      blob = await new Promise<Blob>((ok) => c.toBlob((b) => ok(b!), "image/png"));
    }
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  };

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="writing-panel mindmap-panel" role="dialog" aria-modal="true" aria-label="Mind map">
        <div className="recipe-box-head">
          <Brain size={18} />
          <h2>Mind map</h2>
          <span className="faint small grow">{map ? "Click a branch to fold it." : ""}</span>
          <button className="icon-btn" onClick={() => setZoom(Math.max(0.5, zoom - 0.15))} aria-label="Zoom out"><ZoomOut size={16} /></button>
          <button className="icon-btn" onClick={() => setZoom(Math.min(2, zoom + 0.15))} aria-label="Zoom in"><ZoomIn size={16} /></button>
          <button className="icon-btn" onClick={close} aria-label="Close"><X size={16} /></button>
        </div>
        {error && <div className="banner danger">{error}</div>}
        {!map && !error && (
          <div className="faint" style={{ margin: "auto", padding: 40 }}>
            <Loader2 size={18} className="spin" /> Making the mind map…
          </div>
        )}
        {map && (
          <div className="mindmap-canvas">
            <svg
              ref={svg}
              xmlns="http://www.w3.org/2000/svg"
              viewBox={`${box.x} ${box.y} ${box.w} ${box.h}`}
              width={`${100 * zoom}%`}
              height={`${100 * zoom}%`}
              preserveAspectRatio="xMidYMid meet"
              role="img"
              aria-label={`Mind map of ${map.label}`}
            >
              {nodes
                .filter((n) => n.parent)
                .map((n) => {
                  const p = at.get(n.parent!)!;
                  const mx = (p.x + n.x) / 2;
                  return (
                    <path
                      key={`l${n.id}`}
                      d={`M${p.x},${p.y} C${mx},${p.y} ${mx},${n.y} ${n.x},${n.y}`}
                      fill="none"
                      stroke={`var(--branch-${n.branch % 7})`}
                      strokeWidth={n.depth === 1 ? 3 : 1.6}
                      opacity={0.75}
                    />
                  );
                })}
              {nodes.map((n) => {
                const lines = wrap(n.label, n.depth === 0 ? 18 : 16);
                const w = Math.max(...lines.map((l) => l.length)) * (n.depth === 0 ? 8.4 : 7.2) + 22;
                const h = lines.length * LINE + 14;
                const color = n.depth === 0 ? "var(--accent)" : `var(--branch-${n.branch % 7})`;
                const folded = hidden.has(n.id);
                return (
                  <g
                    key={n.id}
                    transform={`translate(${n.x},${n.y})`}
                    onClick={() => n.hasChildren && n.depth > 0 && toggle(n.id)}
                    style={{ cursor: n.hasChildren && n.depth > 0 ? "pointer" : "default" }}
                  >
                    {n.depth === 1 && <rect x={-w / 2} y={-h / 2} width={w} height={h} rx={9} fill="var(--bg)" />}
                    <rect
                      x={-w / 2}
                      y={-h / 2}
                      width={w}
                      height={h}
                      rx={n.depth === 0 ? 14 : 9}
                      fill={n.depth <= 1 ? color : "var(--surface-2)"}
                      fillOpacity={n.depth === 0 ? 1 : n.depth === 1 ? 0.22 : 1}
                      stroke={color}
                      strokeWidth={n.depth === 0 ? 0 : 1.4}
                      strokeDasharray={folded ? "4 3" : undefined}
                    />
                    {lines.map((l, i) => (
                      <text
                        key={i}
                        x={0}
                        y={(i - (lines.length - 1) / 2) * LINE + 4.5}
                        textAnchor="middle"
                        fontSize={n.depth === 0 ? 14 : 12}
                        fontWeight={n.depth <= 1 ? 650 : 450}
                        fill={n.depth === 0 ? "var(--bg)" : "var(--text)"}
                        fontFamily="system-ui, -apple-system, sans-serif"
                      >
                        {l}
                      </text>
                    ))}
                    {folded && <text x={w / 2 - 4} y={-h / 2 + 2} fontSize={11} fill={color}>+</text>}
                  </g>
                );
              })}
            </svg>
          </div>
        )}
        {map && (
          <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
            <button
              className="btn sm ghost"
              onClick={() => {
                void navigator.clipboard.writeText(toMarkdown(map));
                setCopied(true);
                setTimeout(() => setCopied(false), 1200);
              }}
            >
              <Copy size={13} /> {copied ? "Copied" : "Copy as outline"}
            </button>
            <button className="btn sm ghost" onClick={() => void download("png")}><Download size={13} /> PNG</button>
            <button className="btn sm ghost" onClick={() => void download("svg")}><Download size={13} /> SVG</button>
            {notesOn && (
              <button
                className="btn sm primary"
                onClick={() => {
                  close();
                  openNotes({ draft: { title: `Mind map: ${map.label}`, folder: "Inbox", tags: ["mind map"], body: toMarkdown(map), chat: req.chat } });
                }}
              >
                <NotebookPen size={13} /> Save as note
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
