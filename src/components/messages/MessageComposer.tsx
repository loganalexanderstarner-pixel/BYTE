import {
  Lightbulb,
  Loader2,
  Scissors,
  SpellCheck,
  Undo2,
  Wand2,
} from "lucide-react";
import { useState } from "react";

import { errorText } from "../../lib/api";
import { IDEA_TONES, rewrite, TONES } from "../../lib/composer";

/** A text being written: edit it, fix it, rephrase it in a tone, or pick from a few versions. */
export function MessageComposer({
  value,
  onChange,
  disabled,
  label = "Text",
}: {
  value: string;
  onChange: (v: string) => void;
  disabled?: boolean;
  label?: string;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  const [before, setBefore] = useState<string | null>(null);
  const [ideas, setIdeas] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  const run = async (
    key: string,
    action: Parameters<typeof rewrite>[1],
    tone: string | null = null,
  ) => {
    if (!value.trim() || busy) return;
    setBusy(key);
    setError(null);
    try {
      const next = await rewrite(value, action, tone);
      if (next) {
        setBefore(value);
        onChange(next);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };
  const suggest = async () => {
    if (!value.trim() || busy) return;
    setBusy("ideas");
    setError(null);
    setIdeas([]);
    try {
      const out: string[] = [];
      // One at a time: the model on this Mac writes one thing at once.
      for (const t of IDEA_TONES) {
        const v = await rewrite(value, "tone", t);
        if (v && !out.includes(v)) out.push(v);
        setIdeas([...out]);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };
  const spin = (key: string, Icon: typeof Wand2) =>
    busy === key ? <Loader2 size={13} className="spin" /> : <Icon size={13} />;
  const off = disabled || !!busy;

  return (
    <div className="composer-box">
      <textarea
        className="text-input composer-text"
        rows={3}
        value={value}
        disabled={disabled}
        aria-label={label}
        onChange={(e) => onChange(e.target.value)}
      />
      <div className="composer-tools" role="toolbar" aria-label="Writing tools">
        <button
          className="btn sm ghost"
          disabled={off}
          onClick={() => void run("grammar", "grammar")}
          title="Fix spelling, grammar and punctuation only"
        >
          {spin("grammar", SpellCheck)} Fix grammar &amp; punctuation
        </button>
        <button
          className="btn sm ghost"
          disabled={off}
          onClick={() => void run("rewrite", "rewrite")}
          title="Say it another way"
        >
          {spin("rewrite", Wand2)} Rephrase
        </button>
        {TONES.map((t) => (
          <button
            key={t.id}
            className="pill"
            disabled={off}
            onClick={() => void run(t.id, "tone", t.id)}
            title={`Rephrase it: ${t.label.toLowerCase()}`}
          >
            {busy === t.id && <Loader2 size={11} className="spin" />} {t.label}
          </button>
        ))}
        <button
          className="pill"
          disabled={off}
          onClick={() => void run("shorten", "shorten")}
          title="Make it shorter"
        >
          {spin("shorten", Scissors)} Shorter
        </button>
        <button
          className="btn sm ghost"
          disabled={off}
          onClick={() => void suggest()}
          title="A few versions to pick from"
        >
          {spin("ideas", Lightbulb)} Ideas
        </button>
        {before !== null && (
          <button
            className="btn sm ghost"
            disabled={off}
            onClick={() => (onChange(before), setBefore(null))}
            title="Back to the text before the last change"
          >
            <Undo2 size={13} /> Undo
          </button>
        )}
      </div>
      {ideas.length > 0 && (
        <div className="composer-ideas" aria-label="Versions to pick from">
          {ideas.map((v) => (
            <button
              key={v}
              className="composer-idea"
              disabled={disabled}
              onClick={() => (setBefore(value), onChange(v), setIdeas([]))}
            >
              {v}
            </button>
          ))}
        </div>
      )}
      {error && <div className="hint danger">{error}</div>}
    </div>
  );
}
