import { openUrl } from "@tauri-apps/plugin-opener";
import { Brain, Check, ChevronRight, Copy, RefreshCw, TriangleAlert } from "lucide-react";
import { memo, useMemo, useState, type MouseEvent } from "react";

import { Logo } from "../../design/Logo";
import { duration, tokensPerSec } from "../../lib/format";
import { closeOpenFences, renderMarkdown } from "../../lib/markdown";
import { useStore, type Message } from "../../state/store";

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

function AssistantMessage({ message, isLast, generating }: { message: Message; isLast: boolean; generating: boolean }) {
  const regenerate = useStore((s) => s.regenerate);
  const showStats = useStore((s) => s.settings?.showStats ?? true);
  const [copied, setCopied] = useState(false);
  const html = useMemo(
    () => renderMarkdown(generating ? closeOpenFences(message.content) : message.content),
    [message.content, generating],
  );
  const thinkingLive = generating && !!message.reasoning && message.content.length === 0;
  const waiting = generating && !message.reasoning && message.content.length === 0;
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
      </div>
      {message.reasoning && message.reasoning.trim().length > 0 && (
        <Thinking text={message.reasoning} live={thinkingLive} ms={s?.thinkingMs} />
      )}
      {waiting && (
        <div className="typing" aria-label="BYTE is working">
          <i />
          <i />
          <i />
        </div>
      )}
      {message.content && (
        <div
          className={`prose ${generating ? "cursor" : ""}`}
          onClick={openLinksExternally}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      )}
      {message.status === "error" && (
        <div className="msg-error" role="alert">
          <TriangleAlert size={18} style={{ color: "var(--danger)", flex: "none" }} />
          <div>
            <b>Something went wrong.</b>
            <div className="muted">{message.error}</div>
          </div>
        </div>
      )}
      {message.status === "cancelled" && <div className="faint" style={{ fontSize: "0.85em", marginTop: 6 }}>Stopped.</div>}
      {!generating && (
        <div className={`msg-actions ${isLast ? "visible" : ""}`}>
          {message.content && (
            <button className="icon-btn" onClick={copy} title="Copy">
              {copied ? <Check size={15} /> : <Copy size={15} />}
            </button>
          )}
          {isLast && (
            <button className="icon-btn" onClick={() => void regenerate()} title="Regenerate">
              <RefreshCw size={15} />
            </button>
          )}
          {showStats && s && s.completionTokens > 0 && (
            <span className="stats" title={`${s.promptTokens} prompt tokens · ${s.completionTokens} generated`}>
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
  if (message.role === "user") {
    return (
      <div className="msg user">
        <div className="bubble">{message.content}</div>
      </div>
    );
  }
  return <AssistantMessage message={message} isLast={isLast} generating={generating} />;
});
