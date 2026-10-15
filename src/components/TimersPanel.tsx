import { useEffect, useState } from "react";
import { api, inTauri } from "../lib/api";
import { leftText } from "../lib/timerLeft";
import type { RunningTimer } from "../lib/types";

/** Running timers and alarms, bottom-right, only while there are some. Cancel with the ×. */
export function TimersPanel() {
  const [timers, setTimers] = useState<RunningTimer[]>([]);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!inTauri) return;
    const load = () => void api.timersList().then(setTimers, () => undefined);
    load();
    // New timers are made in a chat turn, so look again when one rings and every few seconds while the app is open.
    const poll = window.setInterval(load, 5000);
    const off = api.onTimerDone(load);
    return () => {
      window.clearInterval(poll);
      void off.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!timers.length) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [timers.length]);

  if (!timers.length) return null;
  return (
    <div className="timers-panel" role="status" aria-label="Running timers">
      {timers.map((t) => (
        <div className="timer-row" key={t.id}>
          <span className="timer-icon" aria-hidden>{t.alarm ? "⏰" : "⏱"}</span>
          <span className="timer-name">{t.label || (t.alarm ? "Alarm" : "Timer")}</span>
          <span className="timer-left">{leftText(t.ends, now)}</span>
          <button
            className="icon-btn sm"
            aria-label={`Cancel ${t.label || "timer"}`}
            onClick={() => void api.timersCancel(t.id).then(() => setTimers((v) => v.filter((x) => x.id !== t.id)))}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
