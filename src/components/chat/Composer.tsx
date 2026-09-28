import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ArrowUp, Brain, Cloud, Images, Loader2, Paperclip, Columns2, Cpu, Gauge, Globe, Rocket, Sparkles, Square, Telescope, Zap } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { inTauri } from "../../lib/api";
import { budgetSummary } from "../../lib/cloudDocs";
import { displayName } from "../../lib/models";
import { AttachmentChips, LibraryPicker } from "./Attachments";
import type { Mode, ThinkingPref } from "../../lib/types";
import { spaceOf, useStore, workspaceOf } from "../../state/store";

const MODES: { id: Mode; label: string; icon: typeof Zap; hint: string }[] = [
  { id: "fast", label: "Fast", icon: Zap, hint: "Quick, short answers. No thinking." },
  { id: "auto", label: "Auto", icon: Gauge, hint: "Thinks only when the question needs it." },
  { id: "deep", label: "Deep", icon: Telescope, hint: "Always thinks first; thorough, structured answers." },
  { id: "extended", label: "Extended", icon: Rocket, hint: "Unlimited thinking and the longest, most complete answers." },
];

/** Icons for the cloud's mode ids; the list itself always comes from the account. */
const CLOUD_ICONS: Record<string, typeof Zap> = { fast: Zap, auto: Gauge, extended: Telescope, extended_plus: Rocket };
const CLOUD_HINTS: Record<string, string> = {
  fast: "Answers immediately, no reasoning.",
  auto: "Picks how deeply to think for each question.",
  extended: "Thinks longer before answering.",
  extended_plus: "No limit on thinking.",
};

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
  const settings = useStore((s) => s.settings);
  const cloudStatus = useStore((s) => s.cloud);
  const updateSettings = useStore((s) => s.updateSettings);
  const privateChat = useStore((s) => s.conversations.find((c) => c.id === s.currentId)?.private ?? false);
  const setWorkspace = useStore((s) => s.setWorkspace);
  const cloudConnected = !!settings?.cloudConnected;
  // A chat stays in the workspace it was started in; a new one uses the current workspace.
  const space = privateChat ? "local" : currentId ? spaceOf(currentId) : workspaceOf(settings);
  // The cloud needs no model on this Mac, so it also works on Macs too small for one.
  const onCloud = cloudConnected && space === "cloud";
  const onBoth = cloudConnected && space === "both";
  const cloudModes = cloudStatus?.account?.modes ?? [];
  const cloudMode = settings?.cloudMode ?? cloudModes[0]?.id;
  const budget = budgetSummary(cloudStatus?.account?.budgets);
  const ready = onCloud || onBoth || (engine.state === "ready" && !tune);
  const pending = useStore((s) => s.pending);
  const attaching = useStore((s) => s.attaching);
  const attachError = useStore((s) => s.attachError);
  const attachFiles = useStore((s) => s.attachFiles);
  const removePending = useStore((s) => s.removePending);
  const [library, setLibrary] = useState(false);
  const savedPrompts = useStore((s) => s.savedPrompts);
  const loadSavedPrompts = useStore((s) => s.loadSavedPrompts);
  // "/" at the start of the message lists saved prompts from the cloud.
  const slash = cloudConnected && /^\/\S*$/.test(text) ? text.slice(1).toLowerCase() : null;
  const slashMatches = slash === null ? [] : (savedPrompts ?? []).filter((p) => p.title.toLowerCase().includes(slash)).slice(0, 8);
  useEffect(() => {
    if (slash !== null) void loadSavedPrompts();
  }, [slash !== null, loadSavedPrompts]);
  const usePrompt = (t: string) => {
    setText(t);
    ref.current?.focus();
  };
  const [dragging, setDragging] = useState(false);

  // Drop photos/files onto the window to attach them (cloud chats).
  useEffect(() => {
    if (!inTauri || !onCloud) return;
    let off: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setDragging(true);
        else if (e.payload.type === "leave") setDragging(false);
        else if (e.payload.type === "drop") {
          setDragging(false);
          if (e.payload.paths.length) void attachFiles(e.payload.paths);
        }
      })
      .then((u) => (off = u));
    return () => off?.();
  }, [onCloud, attachFiles]);

  const pickFiles = async () => {
    const picked = await openDialog({
      multiple: true,
      title: "Attach photos or files",
      filters: [
        { name: "Photos and documents", extensions: ["png", "jpg", "jpeg", "gif", "webp", "heic", "pdf", "docx", "pptx", "xlsx", "txt", "md", "csv"] },
      ],
    });
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (paths.length) await attachFiles(paths);
  };

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
    if (!ready || generating || !text.trim() || attaching > 0) return;
    void send(text);
    setText("");
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (slashMatches.length && (e.key === "Enter" || e.key === "Tab")) {
      e.preventDefault();
      usePrompt(slashMatches[0].text);
      return;
    }
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      submit();
    }
  };

  const placeholder = onCloud
    ? "Ask BYTE anything (answered on your BYTE cloud)…"
    : onBoth
    ? "Ask BYTE anything (this Mac and your cloud both answer)…"
    : ready
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
      {tune && !onCloud && (
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
      {!ready && !tune && engine.state !== "starting" && !onCloud && (
        <div className={`banner ${engine.state === "error" ? "danger" : ""}`}>
          <span className="grow">
            {engine.state === "error" ? engine.message : "BYTE needs a model before it can chat."}
          </span>
          <button className="btn sm primary" onClick={() => openSettings(engine.state === "error" ? "engine" : "models")}>
            {engine.state === "error" ? "Fix it" : "Choose a model"}
          </button>
          <button className="btn sm" onClick={() => (cloudConnected ? void setWorkspace("cloud") : openSettings("cloud"))}>
            <Cloud size={14} /> {cloudConnected ? "Use BYTE Cloud" : "Connect BYTE Cloud"}
          </button>
        </div>
      )}
      <div className={`composer ${dragging ? "dropping" : ""}`}>
        {onCloud && (pending.length > 0 || attaching > 0 || attachError) && (
          <div className="composer-attachments">
            <AttachmentChips items={pending} onRemove={removePending} />
            {attaching > 0 && (
              <span className="faint">
                <Loader2 size={13} className="spin" /> Uploading {attaching}…
              </span>
            )}
            {attachError && <span className="attach-error">{attachError}</span>}
          </div>
        )}
        {library && <LibraryPicker onClose={() => setLibrary(false)} />}
        {slash !== null && (
          <div className="slash-pop" role="listbox" aria-label="Saved prompts">
            {savedPrompts === null && <div className="faint">Loading saved prompts…</div>}
            {savedPrompts !== null && slashMatches.length === 0 && (
              <div className="faint">{savedPrompts.length ? "No saved prompt matches." : "No saved prompts yet (Settings → Cloud → Saved prompts)."}</div>
            )}
            {slashMatches.map((p, i) => (
              <button key={p.id || p.title} role="option" aria-selected={i === 0} className="slash-item" onClick={() => usePrompt(p.text)}>
                <b>/{p.title}</b>
                <span className="faint">{p.text.slice(0, 90)}</span>
              </button>
            ))}
          </div>
        )}
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
          {onCloud && (
            <>
              <button className="icon-btn" onClick={() => void pickFiles()} title="Attach photos or files (or drop them here)" aria-label="Attach">
                <Paperclip size={16} />
              </button>
              <button className="icon-btn" onClick={() => setLibrary(!library)} title="Reuse a photo you've already uploaded" aria-label="Library">
                <Images size={16} />
              </button>
            </>
          )}
          {onCloud ? (
            <div className="segmented" role="group" aria-label="Cloud mode">
              {cloudModes.map(({ id, label }) => {
                const Icon = CLOUD_ICONS[id] ?? Sparkles;
                return (
                  <button
                    key={id}
                    aria-pressed={(settings?.cloudMode ?? cloudModes[0]?.id) === id}
                    onClick={() => void updateSettings({ cloudMode: id })}
                    title={CLOUD_HINTS[id] ? `${label}: ${CLOUD_HINTS[id]}` : label}
                  >
                    <Icon size={14} />
                    {label}
                  </button>
                );
              })}
            </div>
          ) : (
            <div className="segmented" role="group" aria-label="Mode">
              {MODES.map(({ id, label, icon: Icon, hint }) => (
                <button key={id} aria-pressed={mode === id} onClick={() => setMode(id)} title={`${label}: ${hint}`}>
                  <Icon size={14} />
                  {label}
                </button>
              ))}
            </div>
          )}
          {onBoth && cloudModes.length > 0 && (
            <label className="pill model-pick accent" title="The cloud's mode for its answer">
              <Cloud size={14} />
              <select value={cloudMode} onChange={(e) => void updateSettings({ cloudMode: e.target.value })} aria-label="Cloud mode">
                {cloudModes.map(({ id, label }) => (
                  <option key={id} value={id}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          )}
          {(onCloud || onBoth) && budget && (
            <span className="pill cloud-budget" title={budget.detail}>
              <Gauge size={13} /> {budget.short}
            </span>
          )}
          {!onCloud && (
            <button
              className={`pill ${thinking === "on" ? "accent" : ""}`}
              style={{ cursor: "pointer", height: 30 }}
              onClick={() => setThinking(NEXT_THINKING[thinking])}
              title="Auto: think when useful · On: always think first · Off: answer immediately"
            >
              <Brain size={14} />
              {THINKING_LABEL[thinking]}
            </button>
          )}
          {!onCloud && (
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
          )}
          {!onCloud && !onBoth && readyModels.length > 1 && (
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
            <button className="send-btn" onClick={submit} disabled={!ready || !text.trim() || attaching > 0} title="Send (Enter)" aria-label="Send">
              <ArrowUp size={18} />
            </button>
          )}
        </div>
      </div>
      <div className="composer-hint">
        {onCloud
          ? "Answered on your BYTE cloud (falls back to this Mac if it's unreachable)"
          : onBoth
            ? "This Mac and your cloud answer side by side; keep the better one"
            : "Runs entirely on your Mac"} · <kbd>Enter</kbd> to send · <kbd>Shift</kbd>+<kbd>Enter</kbd> for a new line
      </div>
    </div>
  );
}
