import { ArrowUp, Brain, Columns2, Cpu, Gauge, Globe, Rocket, Square, Telescope, Zap } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { displayName } from "../../lib/models";
import type { Mode, ThinkingPref } from "../../lib/types";
import { useStore } from "../../state/store";

const MODES: { id: Mode; label: string; icon: typeof Zap; hint: string }[] = [
  { id: "fast", label: "Fast", icon: Zap, hint: "Quick, short answers. No thinking." },
  { id: "auto", label: "Auto", icon: Gauge, hint: "Thinks only when the question needs it." },
  { id: "deep", label: "Deep", icon: Telescope, hint: "Always thinks first; thorough, structured answers." },
  { id: "extended", label: "Extended", icon: Rocket, hint: "Unlimited thinking and the longest, most complete answers." },
];

const NEXT_THINKING: Record<ThinkingPref, ThinkingPref> = { auto: "on", on: "off", off: "auto" };
const THINKING_LABEL: Record<ThinkingPref, string> = { auto: "Thinking: Auto", on: "Thinking: On", off: "Thinking: Off" };

export function Composer() {
  const [text, setText] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);
  const send = useStore((s) => s.send);
  const stop = useStore((s) => s.stop);
  const generating = useStore((s) => !!s.generating);
  const engine = useStore((s) => s.engine);
  const mode = useStore((s) => s.mode);
  const setMode = useStore((s) => s.setMode);
  const thinking = useStore((s) => s.thinking);
  const setThinking = useStore((s) => s.setThinking);
  const openSettings = useStore((s) => s.openSettings);
  const currentId = useStore((s) => s.currentId);
  const web = useStore((s) => s.settings?.webSearch ?? true);
  const toggleWeb = useStore((s) => s.toggleWeb);
  const loaded = useStore((s) => s.loaded);
  const models = useStore((s) => s.models);
  const answerWith = useStore((s) => s.answerWith);
  const setAnswerWith = useStore((s) => s.setAnswerWith);
  const readyModels = loaded.filter((l) => l.status.state === "ready");
  const tune = useStore((s) => s.tune);
  const ready = engine.state === "ready" && !tune;

  useEffect(() => {
    ref.current?.focus();
  }, [currentId]);

  // Grow with content up to the CSS max-height.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [text]);

  const submit = () => {
    if (!ready || generating || !text.trim()) return;
    void send(text);
    setText("");
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submit();
    }
  };

  const placeholder = ready
    ? "Ask BYTE anything…"
    : tune
      ? "Tuning BYTE for this Mac — one moment…"
      : engine.state === "starting"
      ? "Loading the model — one moment…"
      : engine.state === "noModel"
        ? "Download a model in Settings to start chatting"
        : "The AI engine isn't running — open Settings → Engine";

  return (
    <div className="composer-wrap">
      {tune && (
        <div className="banner tune-banner">
          <span className="grow">
            <b>Tuning BYTE for this Mac</b>
            {tune.modelCount > 1 ? ` (model ${tune.modelIndex} of ${tune.modelCount}, step ${tune.step} of ${tune.total})` : ` (step ${tune.step} of ${tune.total})`}: {tune.label}… This finds the fastest settings for your chip and is saved for next time.
          </span>
          <div className="progress" style={{ width: 120 }}>
            <span
              style={{
                width: `${((Math.max(0, tune.modelIndex - 1) + Math.max(0, tune.step - 1) / Math.max(1, tune.total)) / Math.max(1, tune.modelCount)) * 100}%`,
              }}
            />
          </div>
        </div>
      )}
      {!ready && !tune && engine.state !== "starting" && (
        <div className={`banner ${engine.state === "error" ? "danger" : ""}`}>
          <span className="grow">
            {engine.state === "error" ? engine.message : "BYTE needs a model before it can chat."}
          </span>
          <button className="btn sm primary" onClick={() => openSettings(engine.state === "error" ? "engine" : "models")}>
            {engine.state === "error" ? "Fix it" : "Choose a model"}
          </button>
        </div>
      )}
      <div className="composer">
        <textarea
          ref={ref}
          rows={1}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKey}
          placeholder={placeholder}
          aria-label="Message BYTE"
          spellCheck
        />
        <div className="composer-bar">
          <div className="segmented" role="group" aria-label="Mode">
            {MODES.map(({ id, label, icon: Icon, hint }) => (
              <button key={id} aria-pressed={mode === id} onClick={() => setMode(id)} title={`${label}: ${hint}`}>
                <Icon size={14} />
                {label}
              </button>
            ))}
          </div>
          <button
            className={`pill ${thinking === "on" ? "accent" : ""}`}
            style={{ cursor: "pointer", height: 30 }}
            onClick={() => setThinking(NEXT_THINKING[thinking])}
            title="Auto: think when useful · On: always think first · Off: answer immediately"
          >
            <Brain size={14} />
            {THINKING_LABEL[thinking]}
          </button>
          <button
            className={`pill ${web ? "accent" : ""}`}
            style={{ cursor: "pointer", height: 30 }}
            onClick={toggleWeb}
            aria-pressed={web}
            title={web ? "Web search is on: BYTE searches when a question needs current information" : "Web search is off: BYTE answers from what it already knows"}
          >
            <Globe size={14} />
            {web ? "Web" : "Web off"}
          </button>
          {readyModels.length > 1 && (
            <label className={`pill model-pick ${answerWith !== "main" ? "accent" : ""}`} title="Which loaded model answers">
              {answerWith === "compare" ? <Columns2 size={14} /> : <Cpu size={14} />}
              <select value={answerWith} onChange={(e) => setAnswerWith(e.target.value)} aria-label="Answer with">
                {readyModels.map((l) => (
                  <option key={l.key} value={l.primary ? "main" : l.key}>
                    {displayName(models, l.key)}
                    {l.primary ? " (main)" : ""}
                  </option>
                ))}
                <option value="compare">Compare all</option>
              </select>
            </label>
          )}
          <span className="spacer" />
          {generating ? (
            <button className="send-btn stop" onClick={() => void stop()} title="Stop (Esc)" aria-label="Stop generating">
              <Square size={14} fill="currentColor" />
            </button>
          ) : (
            <button className="send-btn" onClick={submit} disabled={!ready || !text.trim()} title="Send (Enter)" aria-label="Send">
              <ArrowUp size={18} />
            </button>
          )}
        </div>
      </div>
      <div className="composer-hint">
        Runs entirely on your Mac · <kbd>Enter</kbd> to send · <kbd>Shift</kbd>+<kbd>Enter</kbd> for a new line
      </div>
    </div>
  );
}
