import { listen } from "@tauri-apps/api/event";
import { Check, Pause, Play, ShieldAlert, Square, X } from "lucide-react";
import { useEffect, useState } from "react";

import { inTauri } from "../../lib/api";
import { autoApi, feedLine, statusText, type LiveView } from "../../lib/auto";
import { useStore } from "../../state/store";

/** The card for a running autonomous job: progress, a live feed, check-ins that wait for you, and Stop. */
export function AutonomyCard() {
  const [view, setView] = useState<LiveView | null>(null);
  const [report, setReport] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const reloadChats = useStore((s) => s.reloadChats);
  const selectChat = useStore((s) => s.selectChat);

  useEffect(() => {
    if (!inTauri) return;
    autoApi.status().then((s) => s.running && setView(s.view), () => undefined);
    const a = listen<LiveView>("autonomy://event", (e) => setView(e.payload));
    const b = listen<{ chatId: string }>("autonomy://report", (e) => setReport(e.payload.chatId || null));
    return () => {
      void a.then((f) => f());
      void b.then((f) => f());
    };
  }, []);

  if (!view || view.status === "idle") return null;
  const over = ["done", "stopped", "failed"].includes(view.status);
  const feed = open ? view.feed.slice().reverse() : view.feed.slice(-5).reverse();
  return (
    <div role="region" aria-label="Autonomous job" aria-live="polite" className="approval" style={{ position: "fixed", right: 12, bottom: 64, width: 380, maxWidth: "calc(100vw - 24px)", zIndex: 60, background: "var(--panel)" }}>
      <div className="approval-head">
        <ShieldAlert size={16} />
        <b className="grow">{view.title || "Job"}</b>
        {over && (
          <button className="icon-btn" aria-label="Dismiss" onClick={() => setView(null)}>
            <X size={14} />
          </button>
        )}
      </div>
      <small>
        {view.stagesTotal > 0 && `Stage ${Math.max(view.stage, 1)} of ${view.stagesTotal} · `}
        {statusText(view)}
      </small>
      <div role="progressbar" aria-valuenow={view.percent} aria-valuemin={0} aria-valuemax={100} style={{ height: 6, borderRadius: 3, background: "var(--border)", margin: "6px 0" }}>
        <div style={{ width: `${view.percent}%`, height: "100%", borderRadius: 3, background: "var(--accent)" }} />
      </div>
      {view.next && !over && <small>Next: {view.next}</small>}
      {view.checkIn && (
        <div className="approval waiting" style={{ margin: "8px 0" }}>
          <div className="approval-head">
            <b>{view.checkIn.title}</b>
          </div>
          <dl className="approval-fields">
            {view.checkIn.fields.map(([k, v], i) => (
              <div key={i}>
                <dt>{k}</dt>
                <dd>{v}</dd>
              </div>
            ))}
          </dl>
          <div className="approval-actions">
            <button className="btn sm primary" onClick={() => void autoApi.answer(view.checkIn!.id, "yes")}>
              <Check size={13} /> Allow this once
            </button>
            {view.checkIn.canRemember && (
              <button className="btn sm" onClick={() => void autoApi.answer(view.checkIn!.id, "always")}>
                Approve and don't ask again
              </button>
            )}
            <button className="btn sm ghost" onClick={() => void autoApi.answer(view.checkIn!.id, "no")}>
              <X size={13} /> Skip it
            </button>
          </div>
        </div>
      )}
      {feed.length > 0 && (
        <ul className="plain-list" aria-label="Live activity" style={{ maxHeight: open ? 220 : undefined, overflow: "auto", margin: "6px 0" }}>
          {feed.map((f, i) => (
            <li key={i}>
              <small>{feedLine(f)}</small>
            </li>
          ))}
        </ul>
      )}
      <div className="approval-actions">
        {!over && (
          <>
            <button className="btn sm primary" onClick={() => void autoApi.stop()}>
              <Square size={13} /> Stop
            </button>
            {view.status === "paused" ? (
              <button className="btn sm" onClick={() => void autoApi.unpause()}>
                <Play size={13} /> Resume
              </button>
            ) : (
              <button className="btn sm" onClick={() => void autoApi.pause()}>
                <Pause size={13} /> Pause
              </button>
            )}
          </>
        )}
        {view.feed.length > 5 && (
          <button className="btn sm ghost" onClick={() => setOpen(!open)}>
            {open ? "Show less" : "Show all"}
          </button>
        )}
        {over && report && (
          <button className="btn sm primary" onClick={() => void reloadChats().then(() => selectChat(report))}>
            Open the report
          </button>
        )}
      </div>
    </div>
  );
}
