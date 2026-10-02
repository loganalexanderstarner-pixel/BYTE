import { Download, Pause, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { bytes } from "../../lib/format";
import type { VoiceStatus } from "../../lib/types";
import { useStore } from "../../state/store";

/** The speech models: download (with progress), choose, delete. Used in Settings and when the mic is first pressed. */
export function VoiceModels({ compact = false, onReady }: { compact?: boolean; onReady?: () => void }) {
  const [st, setSt] = useState<VoiceStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const downloads = useStore((s) => s.downloads);
  const chosen = useStore((s) => s.settings?.voiceModel ?? "turbo");
  const update = useStore((s) => s.updateSettings);
  const refresh = () => void (inTauri ? api.voiceStatus() : Promise.resolve(null)).then(setSt, (e) => setError(errorText(e)));
  useEffect(refresh, []);
  // A download finished: read the status again.
  const finished = st?.models.filter((m) => downloads[m.key]?.phase === "finished").map((m) => m.id).join(",") ?? "";
  useEffect(() => {
    if (finished) refresh();
  }, [finished]);
  useEffect(() => {
    if (st?.ready) onReady?.();
  }, [st?.ready, onReady]);

  if (!st) return error ? <div className="banner danger">{error}</div> : null;
  return (
    <div className={`voice-models ${compact ? "compact" : ""}`}>
      {error && <div className="banner danger">{error}</div>}
      {st.models.map((m) => {
        const d = downloads[m.key];
        const busy = d && (d.phase === "downloading" || d.phase === "resuming" || d.phase === "verifying");
        const pct = d && d.total ? Math.round((d.bytes / d.total) * 100) : 0;
        return (
          <div key={m.id} className="voice-model">
            <label className="grow" style={{ cursor: m.installed ? "pointer" : "default" }}>
              {m.installed && <input type="radio" name="voice-model" checked={chosen === m.id || (st.ready === m.id && !st.models.some((x) => x.id === chosen && x.installed))} onChange={() => void update({ voiceModel: m.id })} />}{" "}
              <b>{m.name}</b> <span className="faint small">{bytes(m.sizeBytes)}</span>
              {!compact && <small className="muted" style={{ display: "block" }}>{m.about}</small>}
              {busy && (
                <span className="progress" aria-label={`Downloading ${pct}%`}>
                  <span style={{ width: `${pct}%` }} />
                </span>
              )}
              {d?.phase === "failed" && <small className="bad">{d.error}</small>}
            </label>
            {m.installed ? (
              !compact && (
                <button className="icon-btn sm" title="Delete" aria-label={`Delete ${m.name}`} onClick={() => void api.voiceDelete(m.id).then(refresh, (e) => setError(errorText(e)))}>
                  <Trash2 size={13} />
                </button>
              )
            ) : busy ? (
              <button className="btn sm ghost" onClick={() => void api.modelPause(m.key).catch(() => undefined)} title="Pause">
                <Pause size={13} /> {d?.phase === "verifying" ? "Checking…" : `${pct}%`}
              </button>
            ) : (
              <button className="btn sm primary" onClick={() => void api.voiceDownload(m.id).catch((e) => setError(errorText(e)))}>
                <Download size={13} /> {d?.phase === "paused" ? "Resume" : "Download"}
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
