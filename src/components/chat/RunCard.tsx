import { Check, CircleAlert, Circle, Loader2, MessageSquare, RotateCcw, Workflow } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { failedAt, runStatus } from "../../lib/automations";
import type { RunCard as Card, RunView, StepRun } from "../../lib/types";
import { useStore } from "../../state/store";

function StepIcon({ s }: { s: StepRun }) {
  if (s.status === "done") return <Check size={14} className="ok" />;
  if (s.status === "failed") return <CircleAlert size={14} className="bad" />;
  if (s.status === "running") return <Loader2 size={14} className="spin" />;
  return <Circle size={12} className="muted" />;
}

/** An automation running in the background: each step as it goes, then a link to the result. */
export function RunCard({ card }: { card: Card }) {
  const [view, setView] = useState<RunView>({ runId: card.runId, automationId: card.automationId, name: card.name, steps: card.steps, finished: false, ok: false, chat: null });
  const [runId, setRunId] = useState(card.runId);
  const [error, setError] = useState<string | null>(null);
  const selectChat = useStore((s) => s.selectChat);
  const reloadChats = useStore((s) => s.reloadChats);

  useEffect(() => {
    let live = true;
    // A chat opened later: read where the run got to.
    api.automationRunStatus(runId).then((v) => live && v && setView(v), () => {});
    const off = api.onAutomationProgress((v) => {
      if (live && v.runId === runId) setView(v);
    });
    return () => {
      live = false;
      void off.then((f) => f());
    };
  }, [runId]);

  const again = async (from: number) => {
    setError(null);
    try {
      const next = await api.automationRun(view.automationId, from);
      setView((v) => ({ ...v, runId: next, finished: false, ok: false, chat: null, steps: v.steps.map((s, i) => (i >= from ? { ...s, status: "waiting", detail: "" } : s)) }));
      setRunId(next);
    } catch (e) {
      setError(errorText(e));
    }
  };
  const open = async (chat: string) => {
    await reloadChats();
    await selectChat(chat);
  };
  const failed = failedAt(view.steps);

  return (
    <div className={`run-card ${failed != null ? "failed" : view.finished ? "done" : ""}`} role="status" aria-live="polite">
      <div className="run-head">
        <Workflow size={15} />
        <b>{view.name || card.name}</b>
        <span className="muted small">{runStatus(view.steps, view.finished)}</span>
      </div>
      <ol className="run-steps">
        {view.steps.map((s, i) => (
          <li key={i} className={s.status}>
            <StepIcon s={s} />
            <span className="grow">
              {s.label}
              {s.detail && <span className={`small ${s.status === "failed" ? "hint danger" : "muted"}`}> · {s.detail}</span>}
            </span>
          </li>
        ))}
      </ol>
      {error && <div className="hint danger">{error}</div>}
      {(view.finished || failed != null) && (
        <div className="run-actions">
          {view.chat && (
            <button className="btn sm" onClick={() => void open(view.chat!)}>
              <MessageSquare size={13} /> Open the result
            </button>
          )}
          {failed != null && (
            <button className="btn sm ghost" onClick={() => void again(failed)} title="Keeps the steps that worked">
              <RotateCcw size={13} /> Run again from step {failed + 1}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
