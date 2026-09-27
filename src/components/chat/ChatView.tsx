import { useEffect, useRef } from "react";

import { currentConversation, useStore } from "../../state/store";
import { EmptyState } from "./EmptyState";
import { MessageView } from "./MessageView";

export function ChatView() {
  const conv = useStore(currentConversation);
  const generating = useStore((s) => s.generating);
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  // Follow the stream only while the user is at the bottom.
  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  };

  const last = conv?.messages[conv.messages.length - 1];
  const lastLen = (last?.content.length ?? 0) + (last?.reasoning?.length ?? 0);

  useEffect(() => {
    pinned.current = true;
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: "auto" });
  }, [conv?.id]);

  useEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [lastLen, conv?.messages.length]);

  if (!conv || conv.messages.length === 0) {
    return (
      <div className="chat-scroll">
        <div className="chat-column">
          <EmptyState />
        </div>
      </div>
    );
  }

  return (
    <div className="chat-scroll" ref={scroller} onScroll={onScroll}>
      <div className="chat-column" role="log" aria-live="polite">
        {conv.messages.map((m, i) => (
          <MessageView key={m.id} message={m} isLast={i === conv.messages.length - 1} generating={generating === m.id} />
        ))}
      </div>
    </div>
  );
}
