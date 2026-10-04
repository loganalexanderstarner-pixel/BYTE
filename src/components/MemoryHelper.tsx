import { Loader2, RefreshCw, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText, inTauri } from "../lib/api";
import type { MemoryReport } from "../lib/types";

const gb = (b: number) => `${(b / 1e9).toFixed(1)} GB`;

/**
 * Shown when a model couldn't load: which apps are using memory right now,
 * with a Quit button for each (a normal quit, so they can still ask to save)
 * and "Try again" to load the model once there's room.
 */
export function MemoryHelper({ onRetry }: { onRetry(): void }) {
  const [report, setReport] = useState<MemoryReport | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    if (!inTauri) return;
    api.memoryReport().then(setReport).catch(() => setReport(null));
  }, []);
  useEffect(load, [load]);

  const quit = async (name: string) => {
    setBusy(name);
    setError(null);
    try {
      await api.appQuit(name);
      // Give it a moment to exit, then look again.
      setTimeout(load, 1500);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  if (!report || report.apps.length === 0) return null;
  return (
    <div className="memory-helper">
      <div className="row" style={{ gap: 8 }}>
        <span className="grow">
          <b>{gb(report.availableBytes)}</b> of {gb(report.totalBytes)} is free right now. These apps are using the most:
        </span>
        <button className="icon-btn" onClick={load} title="Check again" aria-label="Check memory again">
          <RefreshCw size={14} />
        </button>
      </div>
      <ul>
        {report.apps.map((a) => (
          <li key={a.name}>
            <span className="grow">{a.name}</span>
            <span className="faint">{gb(a.bytes)}</span>
            <button className="btn sm" disabled={!!busy} onClick={() => void quit(a.name)} title={`Quit ${a.name} (it can still ask to save your work)`}>
              {busy === a.name ? <Loader2 size={13} className="spin" /> : <X size={13} />} Quit
            </button>
          </li>
        ))}
      </ul>
      {error && <div className="faint">{error}</div>}
      <button className="btn sm primary" onClick={onRetry}>
        Try loading again
      </button>
    </div>
  );
}
