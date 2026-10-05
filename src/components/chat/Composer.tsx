import { currentDevice, onDevice } from "../../lib/device";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { AppWindow, AudioLines, ArrowUp, Brain, GraduationCap, Cloud, FolderSearch, Images, Loader2, Paperclip, Columns2, Cpu, Gauge, Globe, Rocket, Sparkles, Square, Telescope, Zap, WifiOff } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";

import { api, inTauri } from "../../lib/api";
import { WEB_HINT, WEB_LABEL, webState } from "../../lib/web";
import { budgetSummary } from "../../lib/cloudDocs";
import { displayName } from "../../lib/models";
import { AttachmentChips, LibraryPicker, LocalFileChips } from "./Attachments";
import { MemoryHelper } from "../MemoryHelper";
import { DiagnosticsButton } from "../DiagnosticsButton";
import { MicButton, type MicHandle } from "./MicButton";
import { isStopPhrase, isDone } from "../../lib/handsfree";
import type { Mode, ThinkingPref } from "../../lib/types";
import { canSpeak, spaceOf, useStore, workspaceOf } from "../../state/store";

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

const PHOTO_TYPES = ["png", "jpg", "jpeg", "gif", "webp", "heic", "bmp"];
const CLOUD_DOC_TYPES = ["pdf", "docx", "pptx", "xlsx", "txt", "md", "csv"];
/** What BYTE reads on this Mac (Rust `files::kind_of`). */
const AUDIO_TYPES = ["wav", "mp3", "m4a", "flac", "ogg", "opus", "aac", "aiff", "caf"];
const LOCAL_DOC_TYPES = [
  ...["pdf", "docx", "odt", "pptx", "odp", "xlsx", "ods", "html", "htm", "txt", "md", "csv", "tsv", "json", "yaml", "yml", "xml", "log", "tex"],
  ...["srt", "vtt", "rs", "py", "js", "ts", "tsx", "jsx", "java", "kt", "swift", "c", "h", "cpp", "hpp", "cs", "go", "rb", "php", "sh", "sql", "css"],
];

export function Composer() {
  const [text, setText] = useState("");
  const prefill = useStore((s) => s.prefill);
  useEffect(() => {
    if (prefill === null) return;
    setText(prefill);
    useStore.setState({ prefill: null });
  }, [prefill]);
  const ref = useRef<HTMLTextAreaElement>(null);
  // Voice input: 🎤, or hold Space in an empty box (voice.rs transcribes on this Mac).
  const voiceOn = useStore((s) => s.settings?.voiceEnabled !== false);
  const mic = useRef<MicHandle>(null);
  const holdTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const holding = useRef(false);
  const addSpoken = (spoken: string) => {
    setText((t) => (t.trim() ? `${t.trimEnd()} ${spoken}` : spoken));
    setTimeout(() => ref.current?.focus(), 0);
  };
  // Hands-free conversation (Talk): listen → send → BYTE answers aloud → listen again.
  const talk = useStore((s) => s.talk);
  const setTalk = useStore((s) => s.setTalk);
  const speakingId = useStore((s) => s.speakingId);
  const busy = useStore((s) => s.running.length > 0);
  const lastAnswer = useStore((s) => s.lastAnswer);
  // A spoken turn was sent and BYTE's answer hasn't been heard out yet.
  const awaiting = useRef(false);
  // "Hey BYTE" opened this window: send what's said next, answer out loud, then listen briefly for a follow-up.
  const wakeTurn = useRef(false);
  const followUp = useRef(false);
  const onSpoken = (spoken: string, auto: boolean) => {
    if (auto && wakeTurn.current && !talk) {
      wakeTurn.current = false;
      if (isDone(spoken)) return;
      followUp.current = true;
      void useStore.getState().send(spoken, { spoken: true });
      return;
    }
    if (auto && talk) {
      if (isStopPhrase(spoken)) return setTalk(false);
      awaiting.current = true;
      void useStore.getState().send(spoken);
      return;
    }
    addSpoken(spoken);
  };
  useEffect(() => {
    // A "Hey BYTE" answer has been said: listen about 8 seconds for a follow-up ("and tomorrow?").
    if (followUp.current && !talk && !busy && !speakingId && lastAnswer) {
      followUp.current = false;
      if (!lastAnswer.ok) return;
      wakeTurn.current = true;
      setTimeout(() => void mic.current?.start({ auto: true, waitMs: 8000 }), 250);
    }
  }, [talk, busy, speakingId, lastAnswer]);
  useEffect(() => {
    if (talk) {
      awaiting.current = false;
      void mic.current?.start({ auto: true });
    }
  }, [talk]);
  useEffect(() => {
    // The answer is done and (on a Mac) has been read out: listen for the next turn.
    if (talk && awaiting.current && !busy && !speakingId && lastAnswer) {
      awaiting.current = false;
      if (!lastAnswer.ok) return setTalk(false);
      setTimeout(() => void mic.current?.start({ auto: true }), 250);
    }
  }, [talk, busy, speakingId, lastAnswer, setTalk]);
  useEffect(() => {
    if (!inTauri) return;
    const off = api.onWakeHeard(() => {
      wakeTurn.current = true;
      void mic.current?.start({ auto: true });
    });
    return () => void off.then((f) => f());
  }, []);
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
  const webMode = useStore((s) => webState(s.settings));
  const offline = useStore((s) => !!s.settings?.offline);
  const web = webMode !== "off" && !offline;
  // Agent pill: this message uses the browser ("go to … and fill in …").
  const [browse, setBrowse] = useState(false);
  // Tutor pill: teach step by step in this chat (stays on until turned off or the chat changes).
  const [tutor, setTutor] = useState(false);
  // A byte://ask link (from a Shortcut): the question waits in a new chat's box; it's never sent by itself.
  const newChat = useStore((s) => s.newChat);
  useEffect(() => {
    if (!inTauri) return;
    const off = api.onDeepLinkAsk((t) => {
      newChat();
      setText(t);
      setTimeout(() => ref.current?.focus(), 0);
    });
    return () => void off.then((f) => f());
  }, [newChat]);
  useEffect(() => setTutor(false), [currentId]);
  const toggleWeb = useStore((s) => s.toggleWeb);
  const toggleFiles = useStore((s) => s.toggleFiles);
  const hasFiles = useStore((s) => (s.kb?.sources ?? []).some((x) => x.chunks > 0));
  const filesOn = useStore((s) => !!s.settings?.kbEnabled);
  const loaded = useStore((s) => s.loaded);
  const models = useStore((s) => s.models);
  const hasModel = models.some((m) => m.role === "chat" && m.variants.some((v) => v.installed));
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
  // Files are read on this Mac for local chats; photos only when the loaded model can see.
  const onLocal = !onCloud && !onBoth;
  const canBrowse = onLocal && web && settings?.webAgentEnabled !== false;
  const canTutor = onLocal && settings?.studyEnabled !== false;
  const canSee = engine.state === "ready" && !!engine.vision;
  const cloudModes = cloudStatus?.account?.modes ?? [];
  const cloudMode = settings?.cloudMode ?? cloudModes[0]?.id;
  const budget = budgetSummary(cloudStatus?.account?.budgets);
  const ready = onCloud || onBoth || (engine.state === "ready" && !tune);
  const pending = useStore((s) => s.pending);
  const attaching = useStore((s) => s.attaching);
  const attachError = useStore((s) => s.attachError);
  const attachFiles = useStore((s) => s.attachFiles);
  const removePending = useStore((s) => s.removePending);
  const pendingFiles = useStore((s) => s.pendingFiles);
  const attachLocal = useStore((s) => s.attachLocal);
  const removePendingFile = useStore((s) => s.removePendingFile);
  const attach = onCloud ? attachFiles : attachLocal;
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

  // Drop photos/files onto the window to attach them (cloud and local chats).
  useEffect(() => {
    if (!inTauri || !(onCloud || onLocal)) return;
    let off: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setDragging(true);
        else if (e.payload.type === "leave") setDragging(false);
        else if (e.payload.type === "drop") {
          setDragging(false);
          if (e.payload.paths.length) void attach(e.payload.paths);
        }
      })
      .then((u) => (off = u));
    return () => off?.();
  }, [onCloud, onLocal, attach]);

  const pickFiles = async () => {
    const photos = onCloud || canSee;
    const picked = await openDialog({
      multiple: true,
      title: photos ? "Attach photos or files" : "Attach files",
      filters: [
        { name: photos ? "Photos and documents" : "Documents", extensions: [...(photos ? PHOTO_TYPES : []), ...(onCloud ? CLOUD_DOC_TYPES : LOCAL_DOC_TYPES), ...(onLocal && voiceOn ? AUDIO_TYPES : [])] },
      ],
    });
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (paths.length) await attach(paths);
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
    void send(text, browse && canBrowse ? { task: "browse" } : tutor && canTutor ? { task: "tutor" } : undefined);
    setBrowse(false);
    setText("");
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Hold Space in an empty box to talk (a quick tap does nothing).
    if (voiceOn && e.code === "Space" && !text && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      if (!e.repeat && !holdTimer.current && !holding.current) {
        holdTimer.current = setTimeout(() => {
          holdTimer.current = null;
          holding.current = true;
          void mic.current?.start().then((ok) => (holding.current = ok));
        }, 250);
      }
      return;
    }
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
    ? onDevice("Ask BYTE anything (this Mac and your cloud both answer)…")
    : ready
    ? "Ask BYTE anything…"
    : tune
      ? onDevice("Tuning BYTE for this Mac — one moment…")
      : engine.state === "starting"
      ? "Loading the model — one moment…"
      : engine.state === "noModel"
        ? (hasModel ? "Tap Load it to wake BYTE's model" : "Download a model in Settings to start chatting")
        : "The AI engine isn't running — open Settings → Engine";

  return (
    <div className="composer-wrap">
      {tune && !onCloud && (
        <div className="banner tune-banner">
          <span className="grow">
            <b>{onDevice("Tuning BYTE for this Mac")}</b>
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
            {engine.state === "error" ? engine.message : hasModel ? (currentDevice() === "phone" ? "Your model isn't loaded. Android may close it while BYTE is in the background." : "Your model isn't loaded.") : "BYTE needs a model before it can chat."}
          </span>
          {hasModel && engine.state !== "error" && (
            <button className="btn sm primary" onClick={() => void useStore.getState().wakeEngine(true)}>
              Load it
            </button>
          )}
          <button className={`btn sm ${hasModel && engine.state !== "error" ? "" : "primary"}`} onClick={() => openSettings(engine.state === "error" ? "engine" : "models")}>
            {engine.state === "error" ? "Fix it" : hasModel ? "Models" : "Choose a model"}
          </button>
          {engine.state === "error" && <DiagnosticsButton label="Copy details" />}
          <button className="btn sm" onClick={() => (cloudConnected ? void setWorkspace("cloud") : openSettings("cloud"))}>
            <Cloud size={14} /> {cloudConnected ? "Use BYTE Cloud" : "Connect BYTE Cloud"}
          </button>
        </div>
      )}
      {!ready && !onCloud && engine.state === "error" && /memory|didn't start|keeps stopping/i.test(engine.message) && (
        <div className="banner">
          <MemoryHelper onRetry={() => void api.engineRestart().catch(() => {})} />
        </div>
      )}
      <div className={`composer ${dragging ? "dropping" : ""}`}>
        {(onCloud || onLocal) && (pending.length > 0 || pendingFiles.length > 0 || attaching > 0 || attachError) && (
          <div className="composer-attachments">
            {onCloud && <AttachmentChips items={pending} onRemove={removePending} />}
            {onLocal && <LocalFileChips files={pendingFiles} onRemove={removePendingFile} />}
            {attaching > 0 && (
              <span className="faint">
                <Loader2 size={13} className="spin" /> {onCloud ? "Uploading" : "Reading"} {attaching}…
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
          onKeyUp={(e) => {
            if (e.code !== "Space") return;
            if (holdTimer.current) {
              clearTimeout(holdTimer.current);
              holdTimer.current = null;
            }
            if (holding.current) {
              holding.current = false;
              mic.current?.stop();
            }
          }}
          placeholder={placeholder}
          aria-label="Message BYTE"
          spellCheck
        />
        <div className="composer-bar">
          <div className="composer-tools">
          {voiceOn && <MicButton ref={mic} onText={onSpoken} onNothing={() => {
                wakeTurn.current = false;
                if (talk) setTalk(false);
              }} />}
          {voiceOn && (
            <button
              className={`icon-btn ${talk ? "talk-on" : ""}`}
              onClick={() => setTalk(!talk)}
              aria-pressed={talk}
              title={talk ? "Stop talking with BYTE (Esc)" : canSpeak() ? "Talk with BYTE: say something, BYTE answers out loud, then listens again" : "Talk with BYTE: say something and BYTE answers, then listens again"}
              aria-label="Talk with BYTE"
            >
              <AudioLines size={16} />
            </button>
          )}
          {talk && <span className="talk-pill">{speakingId ? "Speaking…" : busy ? "Thinking…" : "Listening…"}</span>}
          {onLocal && (
            <button
              className="icon-btn"
              onClick={() => void pickFiles()}
              title={canSee ? "Attach photos or files (or drop them here)" : "Attach files: PDF, Word, slides, sheets, text (or drop them here)"}
              aria-label="Attach"
            >
              <Paperclip size={16} />
            </button>
          )}
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
          {!onCloud && offline && (
            <button
              className="pill"
              style={{ cursor: "pointer", height: 30 }}
              onClick={() => void useStore.getState().updateSettings({ offline: false })}
              title="BYTE is offline, so the web isn't used. Click to go back online."
            >
              <WifiOff size={14} />
              Web: Offline
            </button>
          )}
          {!onCloud && !offline && (
            <button
              className={`pill ${web ? "accent" : ""}`}
              style={{ cursor: "pointer", height: 30 }}
              onClick={toggleWeb}
              aria-pressed={web}
              title={WEB_HINT[webMode]}
            >
              <Globe size={14} />
              {WEB_LABEL[webMode]}
            </button>
          )}
          {canTutor && (
            <button
              className={`pill ${tutor ? "accent" : ""}`}
              style={{ cursor: "pointer", height: 30 }}
              onClick={() => setTutor(!tutor)}
              aria-pressed={tutor}
              title={tutor ? "Tutor is on: BYTE teaches step by step and asks you questions instead of just giving answers" : "Tutor: learn step by step (BYTE guides you with questions and hints)"}
            >
              <GraduationCap size={14} />
              Tutor
            </button>
          )}
          {canBrowse && (
            <button
              className={`pill ${browse ? "accent" : ""}`}
              style={{ cursor: "pointer", height: 30 }}
              onClick={() => setBrowse(!browse)}
              aria-pressed={browse}
              title={browse ? "Agent is on for this message: BYTE uses a private browser to do it (it asks before submitting or downloading)" : "Agent: let BYTE use a browser for this message (open sites, click, fill in forms)"}
            >
              <AppWindow size={14} />
              Agent
            </button>
          )}
          {onLocal && hasFiles && (
            <button
              className={`pill ${filesOn ? "accent" : ""}`}
              style={{ cursor: "pointer", height: 30 }}
              onClick={toggleFiles}
              aria-pressed={filesOn}
              title={filesOn ? "My files is on: BYTE searches the folders in Settings → Knowledge base when they may help" : "My files is off: your folders aren't searched"}
            >
              <FolderSearch size={14} />
              {filesOn ? "My files" : "Files off"}
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
          </div>
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
          ? onDevice("Answered on your BYTE cloud (falls back to this Mac if it's unreachable)")
          : onBoth
            ? onDevice("This Mac and your cloud answer side by side; keep the better one")
            : onDevice("Runs entirely on your Mac")} · <kbd>Enter</kbd> to send · <kbd>Shift</kbd>+<kbd>Enter</kbd> for a new line
      </div>
    </div>
  );
}
