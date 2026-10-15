import { Eye, Play, X } from "lucide-react";
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { errorText, inTauri } from "../../lib/api";
import { controlApi, guideStatus, type GuideState } from "../../lib/control";
import type { GuideOffer } from "../../lib/types";

/** Under a how-to answer: BYTE offers to show the steps on the screen as you go, or to do them. Both use the same plan. */
export function GuideCard({ offer }: { offer: GuideOffer }) {
  const [state, setState] = useState<GuideState>("offer");
  const [summary, setSummary] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!inTauri) return;
    const off = listen<{ running: boolean; done: boolean }>("guide://state", (e) => {
      if (!e.payload.running) setState((s) => (s === "showing" ? "done" : s));
    });
    return () => void off.then((f) => f());
  }, []);
  if (state === "dismissed") return null;
  const run = async (f: () => Promise<void>) => {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errorText(e));
      setState("offer");
    }
  };
  const busy = state === "showing" || state === "doing";
  return (
    <div className="mac-card" role="group" aria-label="Guided help">
      <Eye size={15} className="mac-icon" />
      <div className="mac-text">
        <b>Want help with that?</b>
        <ol className="small" style={{ margin: "4px 0", paddingLeft: 18 }}>
          {offer.steps.map((s, i) => (
            <li key={i}>{s.text}</li>
          ))}
        </ol>
        {guideStatus(state, summary) && <span className="muted small">{guideStatus(state, summary)}</span>}
        {error && <span className="hint danger">{error}</span>}
        <div className="row" style={{ gap: 8, marginTop: 6, flexWrap: "wrap" }}>
          <button
            className="btn sm primary"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                setState("showing");
                await controlApi.guideStart(offer.id);
              })
            }
          >
            <Eye size={13} /> Show me as I go
          </button>
          <button
            className="btn sm"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                setState("doing");
                setSummary(await controlApi.guideDo(offer.id));
                setState("done");
              })
            }
          >
            <Play size={13} /> Do it for me
          </button>
          {state === "showing" && (
            <button className="btn sm ghost" onClick={() => void controlApi.guideCommand("stop")}>
              Stop
            </button>
          )}
          {!busy && (
            <button
              className="btn sm ghost"
              onClick={() => {
                void controlApi.guideDismiss(offer.id).catch(() => undefined);
                setState("dismissed");
              }}
            >
              <X size={13} /> No thanks
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
