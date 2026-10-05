import { BETA_NOTE, BetaTag } from "./BetaTag";
import { documentDir, join } from "@tauri-apps/api/path";
import {
  open as openDialog,
  save as saveDialog,
} from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  Download,
  FilePlus2,
  Loader2,
  Paperclip,
  Plus,
  Square,
  Trash2,
  Wand2,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { renderCharts } from "../../lib/docs/charts";
import { renderDoc } from "../../lib/docs/render";
import {
  chartKey,
  DOC_THEMES,
  fileName,
  themeById,
  type ChartImages,
  type DocKind,
  type DocOutline,
  type DocSpec,
  type DocTheme,
} from "../../lib/docs/spec";
import { useStore } from "../../state/store";
import { osText } from "../../lib/platform";

const KINDS: { id: DocKind; label: string; ext: string }[] = [
  { id: "pdf", label: "PDF report", ext: "PDF" },
  { id: "pptx", label: "Slides", ext: "PowerPoint" },
  { id: "docx", label: "Word document", ext: "Word" },
];

type Step =
  | { kind: "form" }
  | { kind: "outline"; outline: DocOutline }
  | {
      kind: "writing";
      outline: DocOutline;
      requestId: string;
      progress: string;
      done: number;
      total: number;
    }
  | { kind: "done"; spec: DocSpec };

/**
 * Documents made with the model on this Mac (docs.rs): plan → the user edits
 * and approves the outline → sections are written → preview → save as PDF,
 * PowerPoint or Word (any of them, from the same written document).
 */
export function LocalDocs() {
  const engine = useStore((s) => s.engine);
  const webOn = useStore((s) => !!s.settings?.webSearch);
  const [step, setStep] = useState<Step>({ kind: "form" });
  const [kind, setKind] = useState<DocKind>("pdf");
  const [prompt, setPrompt] = useState("");
  const [reference, setReference] = useState<string | null>(null);
  const [research, setResearch] = useState(webOn);
  const [themeId, setThemeId] = useState("midnight");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const guard = async (label: string, fn: () => Promise<void>) => {
    setBusy(label);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const plan = () =>
    guard("plan", async () =>
      setStep({
        kind: "outline",
        outline: await api.docOutline(kind, prompt.trim(), reference),
      }),
    );

  const write = (outline: DocOutline) => {
    const requestId = `doc-${Date.now()}`;
    const total = outline.sections.length;
    setStep({
      kind: "writing",
      outline,
      requestId,
      progress: research ? "Getting ready" : "Starting",
      done: 0,
      total,
    });
    void guard("write", async () => {
      try {
        const spec = await api.docWrite(
          {
            requestId,
            kind,
            prompt,
            outline,
            research,
            referencePath: reference,
          },
          (e) =>
            setStep((s) =>
              s.kind !== "writing"
                ? s
                : e.kind === "phase"
                  ? { ...s, progress: e.text }
                  : {
                      ...s,
                      progress: `Writing “${e.title}”`,
                      done: e.index,
                      total: e.total,
                    },
            ),
        );
        setStep({ kind: "done", spec });
      } catch (e) {
        setStep({ kind: "outline", outline });
        throw e;
      }
    });
  };

  if (engine.state !== "ready")
    return (
      <>
        <h3>
          {osText("Make a document on this Mac")}
          <BetaTag />
        </h3>
        <div className="banner">
          {osText("Load a model first (Settings → Models). Documents are written by the model on this Mac.")}
        </div>
      </>
    );

  return (
    <>
      {step.kind === "form" && (
        <>
          <h3>
            {osText("Make a document on this Mac")}
            <BetaTag />
          </h3>
          <p className="muted" style={{ marginTop: 0 }}>
            {osText("Written by the model on this Mac: private, and works offline. BYTE plans an outline first; you edit it before anything is written.")}
          </p>
          <p className="faint small" style={{ marginTop: 0 }}>
            {BETA_NOTE}
          </p>
          {error && <div className="banner danger">{error}</div>}
          <div className="doc-kinds" role="group" aria-label="Document type">
            {KINDS.map((k) => (
              <button
                key={k.id}
                className={`pill ${kind === k.id ? "accent" : ""}`}
                aria-pressed={kind === k.id}
                onClick={() => setKind(k.id)}
              >
                {k.label}
                {k.id === "pptx" && (
                  <BetaTag title="PowerPoint slides still need work: check them before you share them." />
                )}
              </button>
            ))}
          </div>
          <textarea
            className="text-input doc-prompt"
            rows={5}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="What should it be about? e.g. “A short guide to saving for a first home”"
            aria-label="What the document is about"
          />
          <div
            className="row"
            style={{ gap: 10, marginTop: 10, flexWrap: "wrap" }}
          >
            <button
              className="btn sm"
              onClick={async () => {
                const f = await openDialog({
                  title: "Use a document as source material",
                });
                if (typeof f === "string") setReference(f);
              }}
            >
              <Paperclip size={14} /> Source document
            </button>
            {reference && (
              <span className="pill">
                {reference.split(/[\\/]/).pop()}
                <button
                  className="icon-btn"
                  onClick={() => setReference(null)}
                  aria-label="Remove source document"
                >
                  <X size={12} />
                </button>
              </span>
            )}
            <label className="row" style={{ gap: 6, cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={research}
                onChange={(e) => setResearch(e.target.checked)}
              />
              Research the web and cite sources
            </label>
            <ThemePick value={themeId} onChange={setThemeId} />
            <span className="spacer" />
            <button
              className="btn primary"
              disabled={!prompt.trim() || !!busy}
              onClick={() => void plan()}
            >
              {busy === "plan" ? (
                <Loader2 size={14} className="spin" />
              ) : (
                <FilePlus2 size={14} />
              )}{" "}
              {busy === "plan" ? "Planning…" : "Plan it"}
            </button>
          </div>
        </>
      )}

      {step.kind === "outline" && (
        <OutlineEditor
          outline={step.outline}
          error={error}
          busy={!!busy}
          onChange={(outline) => setStep({ kind: "outline", outline })}
          onBack={() => setStep({ kind: "form" })}
          onWrite={write}
        />
      )}

      {step.kind === "writing" && (
        <>
          <h3>{step.outline.title}</h3>
          <div className="job-progress">
            <span>{step.progress}…</span>
            <div className="progress">
              <span
                style={{
                  width: `${Math.round(((step.done + 0.3) / Math.max(1, step.total)) * 100)}%`,
                }}
              />
            </div>
            <span className="faint">
              Section {Math.min(step.done + 1, step.total)} of {step.total}.
              Bigger models write better documents; this can take a few minutes.
            </span>
          </div>
          <button
            className="btn sm"
            onClick={() => void api.chatCancel(step.requestId)}
          >
            <Square size={13} /> Stop
          </button>
        </>
      )}

      {step.kind === "done" && (
        <Finished
          spec={step.spec}
          themeId={themeId}
          onTheme={setThemeId}
          onNew={() => setStep({ kind: "form" })}
        />
      )}
    </>
  );
}

function ThemePick({
  value,
  onChange,
}: {
  value: string;
  onChange(id: string): void;
}) {
  return (
    <label className="row" style={{ gap: 6 }}>
      <span className="faint">Design</span>
      <select
        className="text-input"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        aria-label="Design"
        style={{ width: "auto" }}
      >
        {DOC_THEMES.map((t) => (
          <option key={t.id} value={t.id}>
            {t.label}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Edit the plan: title, subtitle, sections (rename, notes, reorder, add, remove). */
function OutlineEditor({
  outline,
  error,
  busy,
  onChange,
  onBack,
  onWrite,
}: {
  outline: DocOutline;
  error: string | null;
  busy: boolean;
  onChange(o: DocOutline): void;
  onBack(): void;
  onWrite(o: DocOutline): void;
}) {
  const sections = outline.sections;
  const set = (i: number, patch: Partial<DocOutline["sections"][number]>) =>
    onChange({
      ...outline,
      sections: sections.map((s, j) => (j === i ? { ...s, ...patch } : s)),
    });
  const move = (i: number, d: -1 | 1) => {
    if (i + d < 0 || i + d >= sections.length) return;
    const next = [...sections];
    [next[i], next[i + d]] = [next[i + d], next[i]];
    onChange({ ...outline, sections: next });
  };
  return (
    <div className="outline-review">
      <button className="btn sm ghost" onClick={onBack}>
        <ArrowLeft size={14} /> Change the request
      </button>
      <p className="muted">Here's the plan. Edit anything, then write it.</p>
      {error && <div className="banner danger">{error}</div>}
      <input
        className="text-input doc-title-input"
        value={outline.title}
        onChange={(e) => onChange({ ...outline, title: e.target.value })}
        aria-label="Title"
      />
      <input
        className="text-input"
        value={outline.subtitle}
        onChange={(e) => onChange({ ...outline, subtitle: e.target.value })}
        placeholder="Subtitle"
        aria-label="Subtitle"
      />
      <ol className="outline-list">
        {sections.map((s, i) => (
          <li key={i} className="outline-item">
            <div className="grow">
              <input
                className="text-input"
                value={s.title}
                onChange={(e) => set(i, { title: e.target.value })}
                aria-label={`Section ${i + 1}`}
              />
              <input
                className="text-input faint"
                value={s.notes}
                onChange={(e) => set(i, { notes: e.target.value })}
                placeholder="What it covers (optional)"
                aria-label={`Section ${i + 1} notes`}
              />
            </div>
            <button
              className="icon-btn"
              onClick={() => move(i, -1)}
              disabled={i === 0}
              aria-label="Move up"
            >
              <ArrowUp size={14} />
            </button>
            <button
              className="icon-btn"
              onClick={() => move(i, 1)}
              disabled={i === sections.length - 1}
              aria-label="Move down"
            >
              <ArrowDown size={14} />
            </button>
            <button
              className="icon-btn"
              onClick={() =>
                onChange({
                  ...outline,
                  sections: sections.filter((_, j) => j !== i),
                })
              }
              aria-label="Remove section"
            >
              <Trash2 size={14} />
            </button>
          </li>
        ))}
      </ol>
      <button
        className="btn sm ghost"
        onClick={() =>
          onChange({
            ...outline,
            sections: [...sections, { title: "", notes: "" }],
          })
        }
      >
        <Plus size={14} /> Add section
      </button>
      <div className="row" style={{ gap: 8, marginTop: 14 }}>
        <span className="spacer" />
        <button
          className="btn primary"
          disabled={
            busy ||
            !outline.title.trim() ||
            !sections.some((s) => s.title.trim())
          }
          onClick={() =>
            onWrite({
              ...outline,
              sections: sections.filter((s) => s.title.trim()),
            })
          }
        >
          <Wand2 size={14} /> Write it
        </button>
      </div>
    </div>
  );
}

/** Preview, then save in any format. */
function Finished({
  spec,
  themeId,
  onTheme,
  onNew,
}: {
  spec: DocSpec;
  themeId: string;
  onTheme(id: string): void;
  onNew(): void;
}) {
  const theme = themeById(themeId);
  const [charts, setCharts] = useState<ChartImages>({});
  const [busy, setBusy] = useState<DocKind | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void renderCharts(spec, theme)
      .then(setCharts)
      .catch(() => setCharts({}));
  }, [spec, theme]);

  const save = async (kind: DocKind) => {
    setError(null);
    try {
      const dir = await join(await documentDir(), "BYTE");
      const dest = await saveDialog({
        defaultPath: await join(dir, fileName(spec.title, kind)),
      });
      if (!dest) return;
      setBusy(kind);
      await api.docSave(dest, await renderDoc(kind, spec, theme, charts));
      setSaved(dest);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const order = [
    spec.kind,
    ...KINDS.map((k) => k.id).filter((k) => k !== spec.kind),
  ];
  return (
    <>
      <div
        className="row"
        style={{ justifyContent: "space-between", gap: 8, flexWrap: "wrap" }}
      >
        <button className="btn sm ghost" onClick={onNew}>
          <ArrowLeft size={14} /> New document
        </button>
        <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
          <ThemePick value={themeId} onChange={onTheme} />
          {order.map((k, i) => (
            <button
              key={k}
              className={`btn sm ${i === 0 ? "primary" : ""}`}
              disabled={!!busy}
              onClick={() => void save(k)}
            >
              {busy === k ? (
                <Loader2 size={14} className="spin" />
              ) : (
                <Download size={14} />
              )}{" "}
              Save as {KINDS.find((x) => x.id === k)!.ext}
              {k === "pptx" && " (beta)"}
            </button>
          ))}
        </div>
      </div>
      {error && <div className="banner danger">{error}</div>}
      {saved && (
        <div className="banner">
          <span className="grow">Saved to {saved}</span>
          <button
            className="btn sm"
            onClick={() => void revealItemInDir(saved)}
          >
            {osText("Show in Finder")}
          </button>
        </div>
      )}
      <DocPreview spec={spec} theme={theme} charts={charts} />
    </>
  );
}

/** How the document reads, in the chosen design (a page, not the final layout). */
export function DocPreview({
  spec,
  theme,
  charts,
}: {
  spec: DocSpec;
  theme: DocTheme;
  charts: ChartImages;
}) {
  // The document's own colours (its theme), not the app's.
  const vars = {
    "--doc-accent": theme.accent,
    "--doc-heading": theme.heading,
    "--doc-text": theme.text,
    "--doc-muted": theme.muted,
    "--doc-soft": theme.soft,
  } as React.CSSProperties;
  return (
    <article className="doc-preview" style={vars}>
      <header>
        <div className="doc-bar" />
        <h1>{spec.title}</h1>
        {spec.subtitle && <p className="doc-sub">{spec.subtitle}</p>}
      </header>
      {spec.sections.map((s, si) => (
        <section key={si}>
          <h2>{s.title}</h2>
          {s.blocks.map((b, bi) => {
            const k = `${si}-${bi}`;
            switch (b.type) {
              case "paragraph":
                return <p key={k}>{b.text}</p>;
              case "bullets":
                return (
                  <ul key={k}>
                    {b.items.map((x, i) => (
                      <li key={i}>{x}</li>
                    ))}
                  </ul>
                );
              case "numbered":
                return (
                  <ol key={k}>
                    {b.items.map((x, i) => (
                      <li key={i}>{x}</li>
                    ))}
                  </ol>
                );
              case "table":
                return (
                  <table key={k}>
                    <thead>
                      <tr>
                        {b.columns.map((c, i) => (
                          <th key={i}>{c}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {b.rows.map((r, i) => (
                        <tr key={i}>
                          {r.map((c, j) => (
                            <td key={j}>{c}</td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                );
              case "chart":
                return charts[chartKey(si, bi)] ? (
                  <img
                    key={k}
                    className="doc-chart"
                    src={charts[chartKey(si, bi)]}
                    alt={b.title || "Chart"}
                  />
                ) : (
                  <p key={k} className="doc-sub">
                    [Chart: {b.title}]
                  </p>
                );
              case "callout":
                return (
                  <div key={k} className="doc-callout">
                    {b.text}
                  </div>
                );
              case "quote":
                return <blockquote key={k}>{b.text}</blockquote>;
            }
          })}
        </section>
      ))}
      {spec.sources.length > 0 && (
        <section>
          <h2>Sources</h2>
          <ol className="doc-sources">
            {spec.sources.map((s) => (
              <li key={s.n}>
                {s.title || s.url} <span className="doc-sub">{s.url}</span>
              </li>
            ))}
          </ol>
        </section>
      )}
    </article>
  );
}
