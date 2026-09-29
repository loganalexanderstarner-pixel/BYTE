import { Check, Copy, Loader2, PenLine, Square, X } from "lucide-react";
import { useMemo, useRef, useState } from "react";

import { api, errorText } from "../../lib/api";
import { type Action, LANGUAGES, TONES, type Tone, applyResult, changedWords, changes, cleanResult, target } from "../../lib/writing";
import { useStore } from "../../state/store";

const ACTIONS: { id: Action; label: string; hint: string }[] = [
  { id: "rewrite", label: "Rewrite", hint: "Clearer and more natural, same meaning" },
  { id: "shorten", label: "Shorten", hint: "About half as long, every key point kept" },
  { id: "expand", label: "Expand", hint: "About twice as long, more detail and examples" },
  { id: "grammar", label: "Fix grammar", hint: "Spelling, grammar and punctuation only" },
];

/** The writing studio (✍️): rewrite, shorten, expand, change the tone of, or fix a text. */
export function WritingPanel() {
  const initial = useStore((s) => s.writing?.text ?? "");
  const close = useStore((s) => s.closeWriting);
  const [text, setText] = useState(initial);
  const [tone, setTone] = useState<Tone>("friendly");
  const [lang, setLang] = useState<string>("Spanish");
  const [run, setRun] = useState<{ id: string; from: number; to: number; original: string; result: string; done: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);

  const start = async (action: Action) => {
    const el = box.current;
    const { from, to } = target(text, el?.selectionStart ?? 0, el?.selectionEnd ?? 0);
    const original = text.slice(from, to);
    const id = crypto.randomUUID();
    setError(null);
    setRun({ id, from, to, original, result: "", done: false });
    try {
      await api.writingRun(id, original, action, action === "tone" ? tone : action === "translate" ? lang : null, (e) => {
        if (e.kind === "content") setRun((r) => (r && r.id === id ? { ...r, result: r.result + e.delta } : r));
        if (e.kind === "done") setRun((r) => (r && r.id === id ? { ...r, done: true } : r));
      });
    } catch (e) {
      setError(errorText(e));
      setRun((r) => (r && r.id === id ? { ...r, done: true } : r));
    }
  };
  const busy = !!run && !run.done;
  const result = run ? cleanResult(run.result) : "";
  const pieces = useMemo(() => (run?.done ? changes(run.original, result) : []), [run?.done, run?.original, result]);
  const accept = () => {
    if (!run || !result) return;
    setText((t) => applyResult(t, run.from, run.to, result));
    setRun(null);
  };
  const copy = () => {
    void navigator.clipboard.writeText(result).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };
  const words = text.trim() ? text.trim().split(/\s+/).length : 0;

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && !busy && close()}>
      <div className="writing-panel" role="dialog" aria-modal="true" aria-label="Writing studio">
        <div className="recipe-box-head">
          <PenLine size={18} />
          <h2>Writing studio</h2>
          <span className="spacer" />
          <button className="icon-btn" onClick={close} aria-label="Close" disabled={busy}>
            <X size={18} />
          </button>
        </div>
        <div className="writing-tools">
          {ACTIONS.map((a) => (
            <button key={a.id} className="btn sm" title={a.hint} disabled={busy || !text.trim()} onClick={() => void start(a.id)}>
              {a.label}
            </button>
          ))}
          <span className="writing-tone">
            <button className="btn sm" title="Rewrite in the chosen tone" disabled={busy || !text.trim()} onClick={() => void start("tone")}>
              Tone:
            </button>
            <select value={tone} onChange={(e) => setTone(e.target.value as Tone)} aria-label="Tone" disabled={busy}>
              {TONES.map((t) => (
                <option key={t} value={t}>
                  {t[0].toUpperCase() + t.slice(1)}
                </option>
              ))}
            </select>
          </span>
          <span className="writing-tone">
            <button className="btn sm" title="Translate into the chosen language" disabled={busy || !text.trim()} onClick={() => void start("translate")}>
              Translate:
            </button>
            <select value={lang} onChange={(e) => setLang(e.target.value)} aria-label="Language" disabled={busy}>
              {LANGUAGES.map((l) => (
                <option key={l}>{l}</option>
              ))}
            </select>
          </span>
          <span className="spacer" />
          {busy && (
            <button className="btn sm" onClick={() => void api.chatCancel(run!.id)}>
              <Square size={13} /> Stop
            </button>
          )}
        </div>
        {error && <div className="banner error">{error}</div>}
        <div className="writing-body">
          <div className="writing-col">
            <label className="faint small" htmlFor="writing-text">
              Your text · {words} words · select a part to change only that
            </label>
            <textarea
              id="writing-text"
              ref={box}
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="Paste or type what you're writing: an email, an essay, a post…"
              spellCheck
            />
          </div>
          <div className="writing-col">
            <span className="faint small">
              {!run ? "The new version appears here" : busy ? "Writing…" : `${changedWords(pieces)} words changed (highlighted)`}
            </span>
            <div className="writing-result" aria-live="polite">
              {!run ? (
                <p className="muted">Pick what to do with your text. You'll see the new version here before anything changes.</p>
              ) : run.done ? (
                pieces.map((p, i) => (p.added ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>))
              ) : (
                <>
                  {result}
                  <Loader2 size={14} className="spin" />
                </>
              )}
            </div>
            {run?.done && result && (
              <div className="row" style={{ gap: 8, justifyContent: "flex-end" }}>
                <button className="btn sm" onClick={copy}>
                  {copied ? <Check size={14} /> : <Copy size={14} />} Copy
                </button>
                <button className="btn sm" onClick={() => setRun(null)}>
                  Discard
                </button>
                <button className="btn sm primary" onClick={accept}>
                  <Check size={14} /> Use this
                </button>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
