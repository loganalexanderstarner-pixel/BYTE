import { useEffect, useRef } from "react";

import { currentConversation, useStore, type Message } from "../../state/store";
import { EmptyState } from "./EmptyState";
import { MessageView } from "./MessageView";

export function ChatView() {
  const conv = useStore(currentConversation);
  const running = useStore((s) => s.running);
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
        {groupAnswers(conv.messages).map((item, i, all) => {
          const isLast = i === all.length - 1;
          if (Array.isArray(item)) {
            return (
              <div className="compare-grid" key={item[0].id} style={{ gridTemplateColumns: `repeat(${item.length}, minmax(0, 1fr))` }}>
                {item.map((m) => (
                  <MessageView key={m.id} message={m} isLast={isLast} generating={running.includes(m.id)} />
                ))}
              </div>
            );
          }
          return <MessageView key={item.id} message={item} isLast={isLast} generating={running.includes(item.id)} />;
        })}
      </div>
    </div>
  );
}

/** Consecutive answers from the same side-by-side comparison become one row. */
export function groupAnswers(messages: Message[]): (Message | Message[])[] {
  const out: (Message | Message[])[] = [];
  for (const m of messages) {
    const prev = out[out.length - 1];
    if (m.group && Array.isArray(prev) && prev[0].group === m.group) prev.push(m);
    else if (m.group) out.push([m]);
    else out.push(m);
  }
  return out;
}
