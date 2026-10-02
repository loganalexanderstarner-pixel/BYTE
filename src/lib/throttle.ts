import { useEffect, useRef, useState } from "react";

import type { Conversation } from "../state/store";

/**
 * Returns `value`, but while `ms` > 0 updates it at most once every `ms`
 * (the latest value always arrives). Used to re-render streaming answers a
 * dozen times a second instead of on every frame.
 */
export function useThrottled<T>(value: T, ms: number): T {
  const [shown, setShown] = useState(value);
  const last = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    if (ms <= 0) {
      setShown(value);
      return;
    }
    const wait = last.current + ms - Date.now();
    if (wait <= 0) {
      last.current = Date.now();
      setShown(value);
      return;
    }
    clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      last.current = Date.now();
      setShown(value);
    }, wait);
    return () => clearTimeout(timer.current);
  }, [value, ms]);
  return ms <= 0 ? value : shown;
}

/** Everything the sidebar shows about the chats, as one string: it changes
 * only when the list itself does, not with every streamed word. */
export function listSignature(convs: Conversation[]): string {
  return convs
    .map((c) =>
      [c.id, c.title, Math.floor(c.updatedAt / 60_000), c.pinned ? 1 : 0, c.folder ?? "", c.projectId ?? "", c.private ? 1 : 0, c.cloudId ?? "", c.summary ?? "", (c.tags ?? []).join(","), c.messages.length > 0 || (c.messageCount ?? 0) > 0 ? 1 : 0].join("\u0001"),
    )
    .join("\u0002");
}
