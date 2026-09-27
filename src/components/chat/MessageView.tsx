import { openUrl } from "@tauri-apps/plugin-opener";
import { Brain, Check, ChevronLeft, ChevronRight, Cloud, Copy, FastForward, HelpCircle, Layers, Lightbulb, Pencil, RefreshCw, ThumbsDown, ThumbsUp, TriangleAlert } from "lucide-react";
import { memo, useMemo, useState, type MouseEvent } from "react";

import { versionInfo } from "../../lib/branches";

import { Logo } from "../../design/Logo";
import { duration, tokensPerSec } from "../../lib/format";
import { displayName } from "../../lib/models";
import { closeOpenFences, renderMarkdown } from "../../lib/markdown";
import { AttachmentChips } from "./Attachments";
import { useStore, type Message, type Step } from "../../state/store";
import { Activity, Sources } from "./Activity";

function openLinksExternally(e: MouseEvent<HTMLDivElement>) {
  const a = (e.target as HTMLElement).closest("a");
  if (!a) return;
  e.preventDefault();
  const href = a.getAttribute("href");
  if (href && /^https?:\/\//i.test(href)) void openUrl(href);
}

function Thinking({ text, live, ms }: { text: string; live: boolean; ms?: number }) {
  const [open, setOpen] = useState(false);
  const label = live ? "Thinking…" : ms && ms > 0 ? `Thought for ${duration(ms / 1000)}` : "Thought process";
  return (
    <details className="thinking" open={open || undefined} onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}>
      <summary>
        <Brain size={14} className={live ? "dot pulse" : undefined} style={live ? { boxShadow: "none", background: "none", width: 14, height: 14 } : undefined} />
        {label}
        <ChevronRight size={14} style={{ transform: open ? "rotate(90deg)" : undefined, transition: "transform 150ms" }} />
      </summary>
      <div className="body">{text.trim()}</div>
    </details>
  );
}

/** BYTE suggests remembering something; nothing is saved until the user agrees. */
function MemorySuggestion({ messageId, step }: { messageId: string; step: Step }) {
  const resolve = useStore((s) => s.resolveMemory);
  if (!step.summary || step.status !== "ok") return null;
  if (step.decision === "dismissed") return null;
  return (
    <div className="memory-suggest">
      <Lightbulb size={15} style={{ color: "var(--accent)", flex: "none" }} />
      <span className="grow">
        {step.decision === "saved" ? "Saved to memory: " : "Remember this? "}
        <b>{step.summary}</b>
      </span>
      {step.decision !== "saved" && (
        <>
          <button className="btn sm primary" onClick={() => void resolve(messageId, step.id, true)}>Save</button>
          <button className="btn sm ghost" onClick={() => void resolve(messageId, step.id, false)}>No thanks</button>
        </>
      )}
    </div>
  );
}

/** ◀ 2 / 3 ▶ for messages that have other versions. */
function VersionSwitcher({ message }: { message: Message }) {
  const showVersion = useStore((s) => s.showVersion);
  const busy = useStore((s) => s.running.length > 0);
  const { count, index } = versionInfo(message);
  if (count < 2) return null;
  return (
    <span className="versions" aria-label={`Version ${index + 1} of ${count}`}>
      <button className="icon-btn" disabled={busy || index === 0} onClick={() => showVersion(message.id, index - 1)} title="Previous version">
        <ChevronLeft size={14} />
      </button>
      <span>
        {index + 1} / {count}
      </span>
      <button className="icon-btn" disabled={busy || index === count - 1} onClick={() => showVersion(message.id, index + 1)} title="Next version">
        <ChevronRight size={14} />
      </button>
    </span>
  );
}

function UserMessage({ message }: { message: Message }) {
  const editMessage = useStore((s) => s.editMessage);
  const busy = useStore((s) => s.running.length > 0);
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(message.content);

  if (editing) {
    return (
      <div className="msg user editing">
        <textarea
          className="edit-box"
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              setEditing(false);
              void editMessage(message.id, text);
            }
            if (e.key === "Escape") setEditing(false);
          }}
          aria-label="Edit message"
        />
        <div className="row" style={{ gap: 6, justifyContent: "flex-end" }}>
          <button className="btn sm ghost" onClick={() => setEditing(false)}>Cancel</button>
          <button
            className="btn sm primary"
            disabled={!text.trim() || text.trim() === message.content.trim()}
            onClick={() => {
              setEditing(false);
              void editMessage(message.id, text);
            }}
          >
            Send
          </button>
        </div>
      </div>
    );
  }
  return (
    <div className="msg user">
      {message.attachments && <AttachmentChips items={message.attachments} />}
      <div className="bubble">{message.content}</div>
      <div className={`msg-actions user-actions ${versionInfo(message).count > 1 ? "visible" : ""}`}>
        <VersionSwitcher message={message} />
        {!busy && (
          <button
            className="icon-btn"
            title="Edit (keeps the current version)"
            onClick={() => {
              setText(message.content);
              setEditing(true);
            }}
          >
            <Pencil size={14} />
          </button>
        )}
      </div>
    </div>
  );
}

function AssistantMessage({ message, isLast, generating }: { message: Message; isLast: boolean; generating: boolean }) {
  const regenerate = useStore((s) => s.regenerate);
  const cloudAct = useStore((s) => s.cloudAct);
  const cloudModes = useStore((s) => s.cloud?.account?.modes);
  const cloudModeLabel = message.cloudMode ? (cloudModes?.find((m) => m.id === message.cloudMode)?.label ?? message.cloudMode) : null;
  const showStats = useStore((s) => s.settings?.showStats ?? true);
  const models = useStore((s) => s.models);
  // Name the model when it isn't simply the main one.
  const modelLabel = message.model && (message.group || message.picked) ? displayName(models, message.model, true) : null;
  const [copied, setCopied] = useState(false);
  const html = useMemo(
    () => renderMarkdown(generating ? closeOpenFences(message.content) : message.content, message.sources ?? []),
    [message.content, generating, message.sources],
  );
  const toolSteps = (message.steps ?? []).filter((st) => st.name !== "remember");
  const memorySteps = (message.steps ?? []).filter((st) => st.name === "remember");
  const toolRunning = !!message.steps?.some((st) => st.status === "running");
  const thinkingLive = generating && !!message.reasoning && message.content.length === 0 && !toolRunning;
  const waiting = generating && !message.reasoning && message.content.length === 0 && !message.steps?.length;
  const onCloud = !!message.cloud && !!message.remoteId;
  const s = message.stats;

  const copy = async () => {
    await navigator.clipboard.writeText(message.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  return (
    <div className="msg assistant">
      <div className="head">
        <Logo size={16} glow={false} />
        BYTE
        {modelLabel && <span className="model-label">{modelLabel}</span>}
        {message.cloud && (
          <span className="model-label cloud-label" title="Written on your BYTE cloud">
            <Cloud size={12} /> Cloud{cloudModeLabel ? ` · ${cloudModeLabel}` : ""}
          </span>
        )}
      </div>
      {message.notice && <div className="banner notice-banner">{message.notice}</div>}
      {message.reasoning && message.reasoning.trim().length > 0 && (
        <Thinking text={message.reasoning} live={thinkingLive} ms={s?.thinkingMs} />
      )}
      {toolSteps.length > 0 && <Activity steps={toolSteps} live={generating && message.content.length === 0} />}
      {waiting && (
        <div className="typing" aria-label="BYTE is working">
          <i />
          <i />
          <i />
          {message.cloud && <span className="phase">{message.phase ?? "Waiting in line on your BYTE cloud…"}</span>}
        </div>
      )}
      {generating && message.phase && !waiting && <div className="phase live">{message.phase}</div>}
      {generating && onCloud && (
        <button className="btn sm answer-now" onClick={() => void cloudAct(message.id, "answer-now")} title="Stop thinking and answer with what BYTE has so far">
          <FastForward size={13} /> Answer now
        </button>
      )}
      {message.content && (
        <div
          className={`prose ${generating ? "cursor" : ""}`}
          onClick={openLinksExternally}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      )}
      {!generating && message.sources && message.sources.length > 0 && message.content && <Sources sources={message.sources} />}
      {!generating && memorySteps.map((st) => <MemorySuggestion key={st.id} messageId={message.id} step={st} />)}
      {message.status === "error" && (
        <div className="msg-error" role="alert">
          <TriangleAlert size={18} style={{ color: "var(--danger)", flex: "none" }} />
          <div>
            <b>Something went wrong.</b>
            <div className="muted">{message.error}</div>
          </div>
        </div>
      )}
      {message.status === "cancelled" && !message.interrupted && (
        <div className="faint" style={{ fontSize: "0.85em", marginTop: 6 }}>Stopped.</div>
      )}
      {message.interrupted && (
        <div className="interrupted">
          <span>BYTE was closed before finishing this answer.</span>
          {isLast && (
            <button className="btn sm" onClick={() => void regenerate()}>
              <RefreshCw size={13} /> Try again
            </button>
          )}
        </div>
      )}
      {!generating && (
        <div className={`msg-actions ${isLast ? "visible" : ""}`}>
          <VersionSwitcher message={message} />
          {message.content && (
            <button className="icon-btn" onClick={copy} title="Copy">
              {copied ? <Check size={15} /> : <Copy size={15} />}
            </button>
          )}
          {isLast && (
            <button className="icon-btn" onClick={() => void regenerate()} title="Regenerate (keeps this answer as another version)">
              <RefreshCw size={15} />
            </button>
          )}
          {onCloud && message.content && (
            <>
              <button className="icon-btn" onClick={() => void cloudAct(message.id, "deepen")} title="Go deeper: expand this answer">
                <Layers size={15} />
              </button>
              <button className="icon-btn" onClick={() => void cloudAct(message.id, "justify")} title="Explain the reasoning behind this answer">
                <HelpCircle size={15} />
              </button>
              <button
                className={`icon-btn ${message.feedback === "up" ? "on" : ""}`}
                onClick={() => void cloudAct(message.id, "feedback", "up")}
                title="Good answer"
                aria-pressed={message.feedback === "up"}
              >
                <ThumbsUp size={15} />
              </button>
              <button
                className={`icon-btn ${message.feedback === "down" ? "on" : ""}`}
                onClick={() => void cloudAct(message.id, "feedback", "down")}
                title="Bad answer"
                aria-pressed={message.feedback === "down"}
              >
                <ThumbsDown size={15} />
              </button>
            </>
          )}
          {showStats && s && s.completionTokens > 0 && (
            <span
              className="stats"
              title={
                `${s.promptTokens} prompt tokens · ${s.completionTokens} generated` +
                (s.draftTokens > 0 ? ` · Speed boost: ${s.draftAccepted} of ${s.draftTokens} drafted words kept` : "")
              }
            >
              {s.draftTokens > 0 && "⚡ "}
              {tokensPerSec(s.tokensPerSecond)} · {duration(s.totalMs / 1000)}
            </span>
          )}
        </div>
      )}
    </div>
  );
}

export const MessageView = memo(function MessageView({
  message,
  isLast,
  generating,
}: {
  message: Message;
  isLast: boolean;
  generating: boolean;
}) {
  if (message.role === "user") return <UserMessage message={message} />;
  return <AssistantMessage message={message} isLast={isLast} generating={generating} />;
});
