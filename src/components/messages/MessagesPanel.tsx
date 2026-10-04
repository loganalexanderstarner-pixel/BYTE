import {
  Check,
  Loader2,
  MessageCircle,
  RefreshCw,
  Send,
  Sparkles,
  Users,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorText } from "../../lib/api";
import { rewrite } from "../../lib/composer";
import { transcript, when } from "../../lib/messages";
import type { MessageThread, TextMessage } from "../../lib/types";
import { MessageComposer } from "./MessageComposer";

/** The Messages inbox: your conversations, a thread, and a reply you write, fix, rephrase or let BYTE draft. */
export function MessagesPanel({
  onClose,
  open,
  draft,
}: {
  onClose: () => void;
  open?: string | null;
  draft?: boolean;
}) {
  const [threads, setThreads] = useState<MessageThread[] | null>(null);
  const [chat, setChat] = useState<string | null>(open ?? null);
  const [msgs, setMsgs] = useState<TextMessage[] | null>(null);
  const [text, setText] = useState("");
  const [wish, setWish] = useState("");
  const [busy, setBusy] = useState<"draft" | "send" | null>(null);
  const [sent, setSent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const autoDraft = useRef(!!draft);
  const end = useRef<HTMLDivElement | null>(null);

  const loadThreads = useCallback(() => {
    api.messagesThreads().then(
      (t) => {
        setThreads(t);
        setChat((c) => c ?? t[0]?.chat ?? null);
      },
      (e) => setError(errorText(e)),
    );
  }, []);
  const loadThread = useCallback((c: string) => {
    api.messagesThread(c).then(setMsgs, (e) => setError(errorText(e)));
  }, []);
  useEffect(() => {
    loadThreads();
    // New texts arrive while it's open.
    const off = api.onNewTexts(() => {
      loadThreads();
      setChat((c) => (c && loadThread(c), c));
    });
    return () => void off.then((f) => f());
  }, [loadThreads, loadThread]);
  useEffect(() => {
    if (!chat) return;
    setMsgs(null);
    setText("");
    setSent(null);
    loadThread(chat);
  }, [chat, loadThread]);
  useEffect(() => end.current?.scrollIntoView({ block: "end" }), [msgs]);

  const current = threads?.find((t) => t.chat === chat) ?? null;
  const draftReply = useCallback(async () => {
    if (!msgs?.length) return;
    setBusy("draft");
    setError(null);
    try {
      const reply = await rewrite(transcript(msgs, wish), "reply");
      if (reply) setText(reply);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }, [msgs, wish]);
  // Opened from "Draft a reply" on a new text: draft once the thread has loaded.
  useEffect(() => {
    if (autoDraft.current && msgs?.length) {
      autoDraft.current = false;
      void draftReply();
    }
  }, [msgs, draftReply]);

  const send = async () => {
    if (!current || !text.trim()) return;
    setBusy("send");
    setError(null);
    try {
      const how = await api.messagesSend(current.chat, current.handle, text);
      setSent(how);
      setText("");
      setTimeout(() => {
        loadThread(current.chat);
        loadThreads();
      }, 1200);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div
      className="scrim"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        className="study-panel messages-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Messages"
      >
        <div className="recipe-box-head">
          <MessageCircle size={18} />
          <h2>Messages</h2>
          <span className="faint small">On this Mac only</span>
          <span className="spacer" />
          <button
            className="icon-btn"
            onClick={loadThreads}
            aria-label="Refresh"
            title="Refresh"
          >
            <RefreshCw size={16} />
          </button>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        {error && <div className="banner danger">{error}</div>}
        <div className="messages-body">
          <ul className="messages-threads" aria-label="Conversations">
            {threads == null ? (
              <li className="muted">Loading…</li>
            ) : threads.length === 0 ? (
              <li className="muted">No conversations yet.</li>
            ) : (
              threads.map((t) => (
                <li key={t.chat}>
                  <button
                    className={`messages-thread ${t.chat === chat ? "active" : ""} ${t.unread ? "unread" : ""}`}
                    onClick={() => setChat(t.chat)}
                  >
                    <span className="row" style={{ gap: 6 }}>
                      {t.group && <Users size={13} aria-hidden />}
                      <b className="grow ellipsis">{t.name}</b>
                      <span className="faint small">{when(t.lastAt)}</span>
                    </span>
                    <span className="faint small ellipsis">
                      {t.lastFromMe ? "You: " : ""}
                      {t.lastText}
                    </span>
                  </button>
                </li>
              ))
            )}
          </ul>
          <div className="messages-thread-view">
            {!current ? (
              <p className="muted">Pick a conversation.</p>
            ) : (
              <>
                <div className="messages-thread-head">
                  <b>{current.name}</b>
                  {current.handle && current.handle !== current.name && (
                    <span className="faint small"> · {current.handle}</span>
                  )}
                </div>
                <div className="messages-bubbles" aria-live="polite">
                  {msgs == null ? (
                    <p className="muted">Loading…</p>
                  ) : (
                    msgs.map((m, i) => (
                      <div
                        key={i}
                        className={`bubble-row ${m.fromMe ? "me" : "them"}`}
                      >
                        <div
                          className="text-bubble"
                          title={new Date(m.at).toLocaleString()}
                        >
                          {current.group && !m.fromMe && (
                            <span className="faint small">{m.sender}</span>
                          )}
                          {m.text}
                        </div>
                      </div>
                    ))
                  )}
                  <div ref={end} />
                </div>
                <div className="messages-reply">
                  <div className="row" style={{ gap: 6 }}>
                    <input
                      className="text-input grow"
                      value={wish}
                      onChange={(e) => setWish(e.target.value)}
                      placeholder="What do you want to say? (optional, for Draft a reply)"
                      aria-label="What the reply should say"
                    />
                    <button
                      className="btn sm"
                      disabled={!!busy || !msgs?.length}
                      onClick={() => void draftReply()}
                    >
                      {busy === "draft" ? (
                        <Loader2 size={13} className="spin" />
                      ) : (
                        <Sparkles size={13} />
                      )}{" "}
                      Draft a reply
                    </button>
                  </div>
                  <MessageComposer
                    value={text}
                    onChange={(v) => (setText(v), setSent(null))}
                    disabled={busy === "send"}
                    label="Your reply"
                  />
                  <div className="row" style={{ gap: 8 }}>
                    {sent && (
                      <span className="ok small">
                        <Check size={13} /> {sent}
                      </span>
                    )}
                    <button
                      className="btn sm primary"
                      style={{ marginLeft: "auto" }}
                      disabled={!!busy || !text.trim()}
                      onClick={() => void send()}
                    >
                      {busy === "send" ? (
                        <Loader2 size={13} className="spin" />
                      ) : (
                        <Send size={13} />
                      )}{" "}
                      {busy === "send" ? "Sending…" : "Send"}
                    </button>
                  </div>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
