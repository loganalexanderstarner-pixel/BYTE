import { documentDir, join } from "@tauri-apps/api/path";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { ArrowDown, ArrowLeft, ArrowUp, Check, Download, FilePlus2, FileText, Loader2, Paperclip, Plus, RefreshCw, Trash2, Wand2, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorText } from "../../lib/api";
import { DOC_KINDS, idOf, jobState, listOf, readOutline, str, titleOf, writeOutline, type DocKind, type OutlineItem, type Row } from "../../lib/cloudDocs";
import { CloudThumb } from "../chat/Attachments";

type View = { kind: "create" } | { kind: "jobs" } | { kind: "docs" } | { kind: "job"; id: string } | { kind: "doc"; id: string };

/**
 * Documents made on the BYTE cloud (docs/CLOUD-MODE.md → Documents): a job is
 * created, BYTE plans an outline, **the user reviews and approves it** (and
 * picks a design), then the document is written; finished documents can be
 * previewed page by page, downloaded, revised or turned into another format.
 */
export function DocumentsPanel({ onClose }: { onClose(): void }) {
  const [view, setView] = useState<View>({ kind: "create" });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const tabs: { id: "create" | "jobs" | "docs"; label: string }[] = [
    { id: "create", label: "Create" },
    { id: "jobs", label: "In progress" },
    { id: "docs", label: "Documents" },
  ];
  const current = view.kind === "job" ? "jobs" : view.kind === "doc" ? "docs" : view.kind;

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal docs-modal" role="dialog" aria-modal="true" aria-label="Documents">
        <nav className="modal-nav">
          <h2>Documents</h2>
          {tabs.map((t) => (
            <button key={t.id} aria-current={current === t.id} onClick={() => setView({ kind: t.id })}>
              {t.label}
            </button>
          ))}
          <p className="faint" style={{ fontSize: "0.78em", marginTop: "auto" }}>
            Made on your BYTE cloud.
          </p>
        </nav>
        <section className="modal-body">
          <button className="icon-btn modal-close" onClick={onClose} aria-label="Close documents">
            <X size={18} />
          </button>
          {view.kind === "create" && <CreateDoc onCreated={(id) => setView({ kind: "job", id })} />}
          {view.kind === "jobs" && <JobList onOpen={(id) => setView({ kind: "job", id })} />}
          {view.kind === "docs" && <DocList onOpen={(id) => setView({ kind: "doc", id })} />}
          {view.kind === "job" && (
            <JobView id={view.id} onBack={() => setView({ kind: "jobs" })} onDocument={(id) => setView({ kind: "doc", id })} />
          )}
          {view.kind === "doc" && <DocView id={view.id} onBack={() => setView({ kind: "docs" })} onJob={(id) => setView({ kind: "job", id })} />}
        </section>
      </div>
    </div>
  );
}

function CreateDoc({ onCreated }: { onCreated(jobId: string): void }) {
  const [kind, setKind] = useState<DocKind>("pdf");
  const [prompt, setPrompt] = useState("");
  const [reference, setReference] = useState<{ name: string; text: string } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const addReference = async () => {
    const file = await openDialog({ title: "Use a document as source material", filters: [{ name: "Documents", extensions: ["pdf", "docx", "pptx", "txt", "md"] }] });
    if (typeof file !== "string") return;
    setBusy("reference");
    setError(null);
    try {
      // The cloud turns it into text, so only what matters goes into the document.
      const v = await api.cloudUpload<Row | string>("/api/documents/extract-reference", file);
      const text = typeof v === "string" ? v : (str(v.text) ?? str(v.content) ?? JSON.stringify(v));
      setReference({ name: file.split(/[\\/]/).pop() ?? "reference", text });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const create = async () => {
    setBusy("create");
    setError(null);
    try {
      const body: Row = { prompt: prompt.trim(), topic: prompt.trim() };
      if (reference) body.reference = reference.text;
      const job = await api.cloudPost<Row>(`/api/jobs/${kind}`, body);
      const id = idOf(job);
      if (!id) throw new Error("The cloud didn't return a job id.");
      onCreated(id);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <h3>Create a document</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        BYTE plans an outline first. You review it and pick a design before anything is written.
      </p>
      {error && <div className="banner danger">{error}</div>}
      <div className="doc-kinds" role="group" aria-label="Document type">
        {DOC_KINDS.map((k) => (
          <button key={k.id} className={`pill ${kind === k.id ? "accent" : ""}`} aria-pressed={kind === k.id} onClick={() => setKind(k.id)}>
            {k.label}
          </button>
        ))}
      </div>
      <textarea
        className="text-input doc-prompt"
        rows={5}
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        placeholder="What should it be about? e.g. “A 10-slide intro to solar panels for a high-school class”"
        aria-label="What the document is about"
      />
      <div className="row" style={{ gap: 8, marginTop: 10 }}>
        <button className="btn sm" onClick={() => void addReference()} disabled={!!busy}>
          {busy === "reference" ? <Loader2 size={14} className="spin" /> : <Paperclip size={14} />} Source document
        </button>
        {reference && (
          <span className="pill">
            {reference.name} · {reference.text.split(/\s+/).length.toLocaleString()} words
            <button className="icon-btn" onClick={() => setReference(null)} aria-label="Remove source document">
              <X size={12} />
            </button>
          </span>
        )}
        <span className="spacer" />
        <button className="btn primary" disabled={!prompt.trim() || !!busy} onClick={() => void create()}>
          <FilePlus2 size={14} /> {busy === "create" ? "Starting…" : "Plan it"}
        </button>
      </div>
    </>
  );
}

function useList(path: string) {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    setError(null);
    api
      .cloudGet(path)
      .then((v) => setRows(listOf(v)))
      .catch((e) => setError(errorText(e)));
  }, [path]);
  useEffect(load, [load]);
  return { rows, error, load };
}

function JobList({ onOpen }: { onOpen(id: string): void }) {
  const { rows, error, load } = useList("/api/jobs");
  return (
    <>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3>In progress</h3>
        <button className="btn sm" onClick={load}>
          <RefreshCw size={14} /> Refresh
        </button>
      </div>
      {error && <div className="banner danger">{error}</div>}
      {!rows && !error && <Loader2 className="spin" size={16} />}
      {rows?.length === 0 && <p className="faint">No documents being made.</p>}
      <div className="doc-list">
        {rows?.map((r) => {
          const st = jobState(r);
          const id = idOf(r);
          return (
            <button key={id} className="doc-row" onClick={() => id && onOpen(id)}>
              <FileText size={16} />
              <span className="grow">
                <b>{titleOf(r)}</b>
                <span className={`faint job-${st.phase}`}>{st.label}</span>
              </span>
              {st.phase === "approval" && <span className="pill accent">Review</span>}
            </button>
          );
        })}
      </div>
    </>
  );
}

function JobView({ id, onBack, onDocument }: { id: string; onBack(): void; onDocument(id: string): void }) {
  const [job, setJob] = useState<Row | null>(null);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const poll = useCallback(async () => {
    try {
      const j = await api.cloudGet<Row>(`/api/jobs/${id}`);
      setJob(j);
      setError(null);
      const st = jobState(j);
      if (st.phase === "working") timer.current = setTimeout(() => void poll(), 2000);
    } catch (e) {
      setError(errorText(e));
      timer.current = setTimeout(() => void poll(), 5000);
    }
  }, [id]);

  useEffect(() => {
    void poll();
    return () => clearTimeout(timer.current);
  }, [poll]);

  const st = job ? jobState(job) : null;
  return (
    <>
      <button className="btn sm ghost" onClick={onBack}>
        <ArrowLeft size={14} /> All jobs
      </button>
      <h3>{job ? titleOf(job) : "Document"}</h3>
      {error && <div className="banner danger">{error}</div>}
      {!st && <Loader2 className="spin" size={16} />}
      {st?.phase === "working" && (
        <div className="job-progress">
          <span>{st.label}…</span>
          <div className="progress">
            <span className={st.progress === undefined ? "indeterminate" : undefined} style={{ width: `${Math.round((st.progress ?? 0.3) * 100)}%` }} />
          </div>
          <span className="faint">This runs on your cloud; you can close this and come back.</span>
        </div>
      )}
      {st?.phase === "approval" && job && <OutlineReview job={job} id={id} onDone={() => void poll()} onRejected={onBack} />}
      {st?.phase === "done" && (
        <div className="banner">
          <Check size={16} /> <span className="grow">Your document is ready.</span>
          {st.documentId ? (
            <button className="btn sm primary" onClick={() => onDocument(st.documentId!)}>
              Open
            </button>
          ) : (
            <span className="faint">Find it under Documents.</span>
          )}
        </div>
      )}
      {st?.phase === "failed" && <div className="banner danger">{st.label}</div>}
      {st?.phase === "rejected" && <div className="banner">This outline was thrown away.</div>}
    </>
  );
}

/** The approval step: edit the outline, pick a design, then write — or throw it away. */
function OutlineReview({ job, id, onDone, onRejected }: { job: Row; id: string; onDone(): void; onRejected(): void }) {
  const [items, setItems] = useState<OutlineItem[] | null>(null);
  const [template, setTemplate] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const kind = str(job.kind) ?? str(job.type) ?? str(job.format) ?? "pdf";
  const topic = str(job.topic) ?? str(job.prompt) ?? str(job.title) ?? "";

  useEffect(() => {
    api
      .cloudGet(`/api/jobs/${id}/outline`)
      .then((v) => setItems(readOutline(v)))
      .catch((e) => setError(errorText(e)));
  }, [id]);

  const edit = (i: number, title: string) => setItems((xs) => xs && xs.map((x, j) => (j === i ? { ...x, title } : x)));
  const move = (i: number, d: -1 | 1) =>
    setItems((xs) => {
      if (!xs || i + d < 0 || i + d >= xs.length) return xs;
      const next = [...xs];
      [next[i], next[i + d]] = [next[i + d], next[i]];
      return next;
    });

  const approve = async () => {
    if (!items) return;
    setBusy("approve");
    setError(null);
    try {
      const body: Row = { outline: writeOutline(items.filter((x) => x.title.trim())) };
      if (template) body.library_template = template;
      await api.cloudPost(`/api/jobs/${id}/approve`, body);
      onDone();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const reject = async () => {
    setBusy("reject");
    try {
      await api.cloudPost(`/api/jobs/${id}/reject`);
      onRejected();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="outline-review">
      <p className="muted" style={{ marginTop: 0 }}>
        Here's the plan. Rename, reorder, add or remove sections, pick a design, then write it. Throwing it away costs nothing.
      </p>
      {error && <div className="banner danger">{error}</div>}
      {!items && !error && <Loader2 className="spin" size={16} />}
      {items && (
        <ol className="outline-list">
          {items.map((it, i) => (
            <li key={it.key}>
              <input className="text-input" value={it.title} onChange={(e) => edit(i, e.target.value)} aria-label={`Section ${i + 1}`} />
              <button className="icon-btn" onClick={() => move(i, -1)} disabled={i === 0} aria-label="Move up">
                <ArrowUp size={14} />
              </button>
              <button className="icon-btn" onClick={() => move(i, 1)} disabled={i === items.length - 1} aria-label="Move down">
                <ArrowDown size={14} />
              </button>
              <button className="icon-btn" onClick={() => setItems(items.filter((_, j) => j !== i))} aria-label="Remove section">
                <Trash2 size={14} />
              </button>
            </li>
          ))}
        </ol>
      )}
      {items && (
        <button className="btn sm ghost" onClick={() => setItems([...items, { title: "", raw: null, key: `n${Date.now()}` }])}>
          <Plus size={14} /> Add section
        </button>
      )}
      <TemplatePicker kind={kind} topic={topic} value={template} onChange={setTemplate} />
      <div className="row" style={{ gap: 8, marginTop: 14 }}>
        <button className="btn danger" disabled={!!busy} onClick={() => void reject()}>
          <X size={14} /> Throw away
        </button>
        <span className="spacer" />
        <button className="btn primary" disabled={!items?.some((x) => x.title.trim()) || !!busy} onClick={() => void approve()}>
          <Wand2 size={14} /> {busy === "approve" ? "Starting…" : "Approve and write"}
        </button>
      </div>
    </div>
  );
}

function TemplatePicker({ kind, topic, value, onChange }: { kind: string; topic: string; value: string | null; onChange(id: string | null): void }) {
  const { rows } = useList(`/api/templates?kind=${encodeURIComponent(kind)}&topic=${encodeURIComponent(topic.slice(0, 200))}`);
  if (!rows?.length) return null;
  return (
    <div className="template-picker">
      <h4>Design</h4>
      <div className="template-grid">
        <button className={`template-card ${value === null ? "on" : ""}`} onClick={() => onChange(null)} aria-pressed={value === null}>
          <span className="thumb builtin">Aa</span>
          <span className="name">Built-in designs</span>
        </button>
        {rows.map((t) => {
          const id = idOf(t);
          if (!id) return null;
          return (
            <button key={id} className={`template-card ${value === id ? "on" : ""}`} onClick={() => onChange(id)} aria-pressed={value === id} title={titleOf(t)}>
              <CloudThumb className="thumb" path={`/api/templates/${id}/thumb`} alt={titleOf(t)} />
              <span className="name">{titleOf(t)}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function DocList({ onOpen }: { onOpen(id: string): void }) {
  const { rows, error, load } = useList("/api/documents");
  return (
    <>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h3>Documents</h3>
        <button className="btn sm" onClick={load}>
          <RefreshCw size={14} /> Refresh
        </button>
      </div>
      {error && <div className="banner danger">{error}</div>}
      {!rows && !error && <Loader2 className="spin" size={16} />}
      {rows?.length === 0 && <p className="faint">No documents yet. Create one to get started.</p>}
      <div className="doc-list">
        {rows?.map((r) => {
          const id = idOf(r);
          return (
            <button key={id} className="doc-row" onClick={() => id && onOpen(id)}>
              <FileText size={16} />
              <span className="grow">
                <b>{titleOf(r)}</b>
                <span className="faint">{(str(r.format) ?? str(r.kind) ?? "").toUpperCase()}</span>
              </span>
            </button>
          );
        })}
      </div>
    </>
  );
}

const FORMATS = ["pdf", "docx", "pptx"];

function DocView({ id, onBack, onJob }: { id: string; onBack(): void; onJob(id: string): void }) {
  const [doc, setDoc] = useState<Row | null>(null);
  const [pages, setPages] = useState<number | null>(null);
  const [base, setBase] = useState(1);
  const [shown, setShown] = useState(6);
  const [instruction, setInstruction] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  useEffect(() => {
    api
      .cloudGet<Row>("/api/documents")
      .then((v) => setDoc(listOf(v).find((r) => idOf(r) === id) ?? null))
      .catch(() => {});
    api
      .cloudGet<Row | number>(`/api/documents/${id}/preview`)
      .then(async (v) => {
        const n = typeof v === "number" ? v : Number(v.pages ?? v.page_count ?? v.count ?? 0);
        setPages(n || 0);
        // Pages are numbered from 1 unless the cloud says otherwise.
        await api.cloudImage(`/api/documents/${id}/preview/1`).catch(async () => {
          await api.cloudImage(`/api/documents/${id}/preview/0`);
          setBase(0);
        });
      })
      .catch(() => setPages(0));
  }, [id]);

  const format = (str(doc?.format) ?? str(doc?.kind) ?? "pdf").toLowerCase();
  const title = doc ? titleOf(doc) : "Document";

  const download = async () => {
    setError(null);
    try {
      const dir = await join(await documentDir(), "BYTE");
      const dest = await saveDialog({ defaultPath: await join(dir, `${title.replace(/[/\\:*?"<>|]/g, "-")}.${format}`) });
      if (!dest) return;
      setBusy("download");
      await api.cloudDownload(`/api/documents/${id}/download`, dest);
      setSaved(dest);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const followUp = async (label: string, path: string, body: Row) => {
    setBusy(label);
    setError(null);
    try {
      const v = await api.cloudPost<Row>(path, body);
      const job = str(v.job_id) ?? (str(v.status) ? idOf(v) : undefined);
      if (job) onJob(job);
      else setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <button className="btn sm ghost" onClick={onBack}>
        <ArrowLeft size={14} /> All documents
      </button>
      <div className="row" style={{ justifyContent: "space-between", gap: 8 }}>
        <h3>{title}</h3>
        <button className="btn primary" onClick={() => void download()} disabled={!!busy}>
          <Download size={14} /> {busy === "download" ? "Saving…" : `Download .${format}`}
        </button>
      </div>
      {error && <div className="banner danger">{error}</div>}
      {saved && (
        <div className="banner">
          <span className="grow">Saved to {saved}</span>
          <button className="btn sm" onClick={() => void revealItemInDir(saved)}>
            Show in Finder
          </button>
        </div>
      )}
      {pages === null && <Loader2 className="spin" size={16} />}
      {pages !== null && pages > 0 && (
        <>
          <div className="page-grid">
            {Array.from({ length: Math.min(pages, shown) }, (_, i) => (
              <CloudThumb key={i} className={`page ${format === "pptx" ? "slide" : ""}`} path={`/api/documents/${id}/preview/${i + base}`} alt={`Page ${i + 1}`} />
            ))}
          </div>
          {pages > shown && (
            <button className="btn sm ghost" onClick={() => setShown(shown + 12)}>
              Show more pages ({pages - shown} more)
            </button>
          )}
        </>
      )}
      <div className="section">
        <h4>Change it</h4>
        <div className="row" style={{ gap: 8 }}>
          <input
            className="text-input"
            style={{ flex: 1 }}
            value={instruction}
            onChange={(e) => setInstruction(e.target.value)}
            placeholder="e.g. “Make it shorter and add a summary slide”"
            aria-label="What to change"
          />
          <button className="btn" disabled={!instruction.trim() || !!busy} onClick={() => void followUp("revise", `/api/documents/${id}/revise`, { instruction: instruction.trim() })}>
            <Wand2 size={14} /> Revise
          </button>
        </div>
        <div className="row" style={{ gap: 8, marginTop: 10 }}>
          <span className="faint">Same content as</span>
          {FORMATS.filter((f) => f !== format).map((f) => (
            <button key={f} className="btn sm" disabled={!!busy} onClick={() => void followUp(`render-${f}`, `/api/documents/${id}/render`, { format: f })}>
              .{f}
            </button>
          ))}
        </div>
      </div>
    </>
  );
}
