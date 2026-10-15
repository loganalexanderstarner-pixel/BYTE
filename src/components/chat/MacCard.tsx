import { Check, CircleAlert, Undo2 } from "lucide-react";
import { useState } from "react";

import { api, errorText } from "../../lib/api";
import type { MacDone } from "../../lib/types";

/** What BYTE did in a Mac app, with Undo for notes, reminders and events it added. */
export function MacCard({ done }: { done: MacDone }) {
  const [state, setState] = useState<"idle" | "busy" | "undone" | "gone">("idle");
  const [error, setError] = useState<string | null>(null);
  const undo = async () => {
    if (!done.undo) return;
    setState("busy");
    setError(null);
    try {
      setState((await api.macUndo(done.undo)) ? "undone" : "gone");
    } catch (e) {
      setError(errorText(e));
      setState("idle");
    }
  };
  return (
    <div className={`mac-card ${done.ok ? "" : "failed"} ${state === "undone" ? "undone" : ""}`} role="status">
      {done.ok ? <Check size={15} className="mac-icon" /> : <CircleAlert size={15} className="mac-icon" />}
      <div className="mac-text">
        <b>{state === "undone" ? `Undone: ${done.title}` : done.title}</b>
        <span className="muted small">
          {done.app}
          {done.detail && ` · ${done.detail}`}
        </span>
        {error && <span className="hint danger">{error}</span>}
      </div>
      {done.ok && done.undo && state !== "undone" && (
        <button className="btn sm ghost" disabled={state !== "idle"} onClick={() => void undo()} title={state === "gone" ? "BYTE can't undo this any more" : `Remove it from ${done.app}`}>
          <Undo2 size={13} /> {state === "gone" ? "Can't undo now" : "Undo"}
        </button>
      )}
    </div>
  );
}
