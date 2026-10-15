import { useEffect, useState } from "react";

import { api, inTauri } from "../lib/api";
import type { ScreenRecording } from "../lib/types";

/** The words on the badge, or null when nothing is being kept (the badge is then not shown). */
export function badgeText(s: ScreenRecording | undefined): string | null {
  if (!s?.on) return null;
  if (s.recording) return `BYTE is keeping the last ${s.seconds} seconds of your screen in memory`;
  return `Screen replay paused${s.paused ? ` (${s.paused})` : ""}`;
}

/** Shown in the app while the screen replay is on: what is kept, that it stays in memory, and a button to forget it. */
export function ReplayBadge() {
  const [state, setState] = useState<ScreenRecording | undefined>();
  useEffect(() => {
    if (!inTauri) return;
    void api.screenStatus().then((i) => setState(i.status), () => undefined);
    const off = api.onScreenRecording(setState);
    return () => void off.then((f) => f());
  }, []);
  const text = badgeText(state);
  if (!text) return null;
  return (
    <div
      role="status"
      aria-label="Screen replay"
      style={{ position: "fixed", left: 12, bottom: 12, zIndex: 50, display: "flex", gap: 8, alignItems: "center", padding: "6px 10px", borderRadius: 10, background: "var(--panel, #222)", color: "var(--text, #eee)", border: "1px solid var(--border, #444)", fontSize: 12 }}
    >
      <span aria-hidden style={{ color: state?.recording ? "#e5484d" : "#999" }}>●</span>
      <span>{text}</span>
      <button className="btn sm" onClick={() => void api.screenClear()}>Forget it now</button>
    </div>
  );
}
