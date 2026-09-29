import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { documentDir, join } from "@tauri-apps/api/path";
import { ArrowDown, ArrowUp, Check, Copy, Loader2, Plus, Save, Sparkles, Square, Trash2 } from "lucide-react";
import { useState } from "react";

import { api, errorText } from "../../lib/api";
import { KINDS, POEM_FORMS, type Kind, type LongAsk, type Outline, fileName, toBase64 } from "../../lib/writing";
import { useStore } from "../../state/store";

/** "Teach BYTE your style": paste samples, get a style profile (editable, clearable). */
export function StyleSetup({ onDone }: { onDone: () => void }) {
  const style = useStore((s) => s.settings?.writingStyle ?? "");
  const update = useStore((s) => s.updateSettings);
  const [samples, setSamples] = useState(["", "", ""]);
  const [profile, setProfile] = useState(style);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const learn = async () => {
    setBusy(true);
    setError(null);
    try {
      setProfile(await api.styleLearn(samples.filter((s) => s.trim())));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="style-setup">
      <p className="muted small">
        Paste one to three things you wrote yourself (emails, posts, an essay; a few paragraphs each). BYTE describes how you write, and "Write like
        me" uses it. The samples aren't kept.
      </p>
      {samples.map((s, i) => (
        <textarea key={i} rows={4} value={s} placeholder={`Sample ${i + 1}${i ? " (optional)" : ""}`} onChange={(e) => setSamples(samples.map((x, k) => (k === i ? e.target.value : x)))} />
      ))}
      <div className="row" style={{ gap: 8 }}>
        <button className="btn sm primary" disabled={busy || !samples.some((s) => s.trim().split(/\s+/).length >= 30)} onClick={() => void learn()}>
          {busy ? <Loader2 size={14} className="spin" /> : <Sparkles size={14} />} Learn my style
        </button>
        <span className="faint small">Needs about 30 words or more.</span>
      </div>
      {error && <div className="banner danger">{error}</div>}
      <label className="faint small">
        Your style (edit it if something's off)
        <textarea rows={6} value={profile} onChange={(e) => setProfile(e.target.value)} placeholder="Nothing learned yet." />
      </label>
      <div className="row" style={{ gap: 8, justifyContent: "flex-end" }}>
        {style && (
          <button className="btn sm" onClick={() => void update({ writingStyle: "" }).then(onDone)}>
            <Trash2 size={14} /> Forget my style
          </button>
        )}
        <button className="btn sm" onClick={onDone}>
          Cancel
        </button>
        <button className="btn sm primary" disabled={!profile.trim()} onClick={() => void update({ writingStyle: profile.trim() }).then(onDone)}>
          Save
        </button>
      </div>
    </div>
  );
}

type Stage = { kind: "form" } | { kind: "outline"; outline: Outline } | { kind: "writing"; outline: Outline; part: number; text: string; done: boolean; id: string };

/** "Write something new": plan an essay, story, post, report or speech, then write it part by part; or a poem. */
export function Longform({ likeMe, onEdit }: { likeMe: boolean; onEdit: (text: string) => void }) {
  const [ask, setAsk] = useState<LongAsk>({ kind: "essay", topic: "", words: 800, minutes: 5, form: "free verse", notes: "", likeMe });
  const [stage, setStage] = useState<Stage>({ kind: "form" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const a = { ...ask, likeMe };
  const set = <K extends keyof LongAsk>(k: K, v: LongAsk[K]) => setAsk({ ...ask, [k]: v });

  const plan = async () => {
    setBusy(true);
    setError(null);
    try {
      const outline = await api.writingOutline(a);
      if (a.kind === "poem") void write(outline);
      else setStage({ kind: "outline", outline });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const write = async (outline: Outline) => {
    const parts = a.kind === "poem" ? 1 : outline.sections.length;
    let text = "";
    for (let part = 0; part < parts; part++) {
      const id = crypto.randomUUID();
      setStage({ kind: "writing", outline, part, text, done: false, id });
      const before = text;
      let chunk = "";
      let stopped = false;
      try {
        await api.writingSection(id, a, outline, part, before, (e) => {
          if (e.kind === "content") {
            chunk += e.delta;
            setStage((s) => (s.kind === "writing" && s.id === id ? { ...s, text: before + chunk } : s));
          }
          if (e.kind === "done" && e.finishReason === "cancelled") stopped = true;
        });
      } catch (e) {
        setError(errorText(e));
        stopped = true;
      }
      text = `${before}${chunk.trim()}\n\n`;
      if (stopped) break;
    }
    setStage({ kind: "writing", outline, part: parts, text: text.trim(), done: true, id: "" });
  };
  const outlineEdit = (o: Outline) => setStage({ kind: "outline", outline: o });
  const copy = (t: string) =>
    void navigator.clipboard.writeText(t).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  const saveDoc = async (t: string, title: string) => {
    try {
      const path = await saveDialog({ defaultPath: await join(await documentDir(), fileName(title || a.topic)), filters: [{ name: "Markdown", extensions: ["md"] }] });
      if (path) await api.docSave(path, toBase64(t));
    } catch (e) {
      setError(errorText(e));
    }
  };

  if (stage.kind === "outline") {
    const o = stage.outline;
    const move = (i: number, d: number) => {
      const s = [...o.sections];
      [s[i], s[i + d]] = [s[i + d], s[i]];
      outlineEdit({ ...o, sections: s });
    };
    return (
      <div className="longform">
        <label className="faint small">
          Title
          <input value={o.title} onChange={(e) => outlineEdit({ ...o, title: e.target.value })} />
        </label>
        <p className="faint small" style={{ margin: 0 }}>
          The plan: change, reorder or remove parts, then write it.
        </p>
        <ol className="outline-list">
          {o.sections.map((s, i) => (
            <li key={i}>
              <input value={s.heading} onChange={(e) => outlineEdit({ ...o, sections: o.sections.map((x, k) => (k === i ? { ...x, heading: e.target.value } : x)) })} />
              <span className="muted small">{s.points.join(" · ")}</span>
              <span className="row" style={{ gap: 2 }}>
                <button className="icon-btn" aria-label="Move up" disabled={i === 0} onClick={() => move(i, -1)}>
                  <ArrowUp size={14} />
                </button>
                <button className="icon-btn" aria-label="Move down" disabled={i === o.sections.length - 1} onClick={() => move(i, 1)}>
                  <ArrowDown size={14} />
                </button>
                <button className="icon-btn" aria-label="Remove" disabled={o.sections.length <= 2} onClick={() => outlineEdit({ ...o, sections: o.sections.filter((_, k) => k !== i) })}>
                  <Trash2 size={14} />
                </button>
              </span>
            </li>
          ))}
        </ol>
        <div className="row" style={{ gap: 8, justifyContent: "flex-end" }}>
          <button className="btn sm" onClick={() => outlineEdit({ ...o, sections: [...o.sections, { heading: "New part", points: [] }] })}>
            <Plus size={14} /> Add a part
          </button>
          <button className="btn sm" onClick={() => setStage({ kind: "form" })}>
            Back
          </button>
          <button className="btn sm primary" onClick={() => void write(o)}>
            Write it
          </button>
        </div>
        {error && <div className="banner danger">{error}</div>}
      </div>
    );
  }

  if (stage.kind === "writing") {
    const parts = a.kind === "poem" ? 1 : stage.outline.sections.length;
    return (
      <div className="longform">
        <span className="faint small">
          {stage.done ? `Done · ${stage.text.split(/\s+/).filter(Boolean).length} words` : `Writing part ${stage.part + 1} of ${parts}…`}
        </span>
        <div className="writing-result">{stage.text}</div>
        <div className="row" style={{ gap: 8, justifyContent: "flex-end" }}>
          {!stage.done ? (
            <button className="btn sm" onClick={() => void api.chatCancel(stage.id)}>
              <Square size={13} /> Stop
            </button>
          ) : (
            <>
              <button className="btn sm" onClick={() => setStage({ kind: "form" })}>
                Start over
              </button>
              <button className="btn sm" onClick={() => void saveDoc(stage.text, stage.outline.title)}>
                <Save size={14} /> Save
              </button>
              <button className="btn sm" onClick={() => copy(stage.text)}>
                {copied ? <Check size={14} /> : <Copy size={14} />} Copy
              </button>
              <button className="btn sm primary" onClick={() => onEdit(stage.text)}>
                Edit it
              </button>
            </>
          )}
        </div>
        {error && <div className="banner danger">{error}</div>}
      </div>
    );
  }

  return (
    <div className="longform">
      <div className="kind-row" role="radiogroup" aria-label="What to write">
        {KINDS.map(([k, label]) => (
          <button key={k} className={`btn sm ${ask.kind === k ? "primary" : ""}`} role="radio" aria-checked={ask.kind === k} onClick={() => set("kind", k as Kind)}>
            {label}
          </button>
        ))}
      </div>
      <label className="faint small">
        About
        <textarea rows={3} value={ask.topic} onChange={(e) => set("topic", e.target.value)} placeholder="e.g. why teenagers need more sleep; a toast for my sister's wedding; a mystery on a night train" />
      </label>
      <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
        {ask.kind === "speech" ? (
          <label className="faint small">
            Minutes out loud
            <input type="number" min={1} max={30} value={ask.minutes ?? 5} onChange={(e) => set("minutes", Number(e.target.value))} />
          </label>
        ) : ask.kind === "poem" ? (
          <label className="faint small">
            Form
            <select value={ask.form ?? "free verse"} onChange={(e) => set("form", e.target.value)}>
              {POEM_FORMS.map((f) => (
                <option key={f}>{f}</option>
              ))}
            </select>
          </label>
        ) : (
          <label className="faint small">
            About how many words
            <select value={ask.words ?? 800} onChange={(e) => set("words", Number(e.target.value))}>
              {[300, 500, 800, 1200, 2000, 3000].map((w) => (
                <option key={w} value={w}>
                  {w.toLocaleString()}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      <label className="faint small">
        Anything else (optional)
        <textarea rows={2} value={ask.notes} onChange={(e) => set("notes", e.target.value)} placeholder="Who it's for, points to include, the tone you want…" />
      </label>
      <div className="row" style={{ gap: 8, justifyContent: "flex-end" }}>
        <button className="btn sm primary" disabled={busy || !ask.topic.trim()} onClick={() => void plan()}>
          {busy && <Loader2 size={14} className="spin" />} {ask.kind === "poem" ? "Write it" : "Plan it"}
        </button>
      </div>
      {error && <div className="banner danger">{error}</div>}
    </div>
  );
}
