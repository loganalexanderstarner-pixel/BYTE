import { ArrowDown, ArrowUp, Link2, Pencil, Play, Plus, Trash2, Workflow, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { STEP_KINDS, blankAutomation, moveStep, newStep, problem, runStatus, stepLabel } from "../../lib/automations";
import { dueText } from "../../lib/tasks";
import type { AutoStep, Automation, RunView, ShortcutMade } from "../../lib/types";
import { useStore } from "../../state/store";

type When = "manual" | "launch" | "schedule";

const whenOf = (trigger: string): When => (trigger === "manual" || trigger === "launch" ? trigger : "schedule");

/** One step's fields in the builder. */
function StepFields({ step, onChange }: { step: AutoStep; onChange: (s: AutoStep) => void }) {
  switch (step.type) {
    case "ask":
      return <input className="grow" value={step.prompt} onChange={(e) => onChange({ ...step, prompt: e.target.value })} placeholder="e.g. find the latest AI news, or: summarize it in 5 bullets" aria-label="What to ask BYTE" />;
    case "notify":
      return <input className="grow" value={step.title} onChange={(e) => onChange({ ...step, title: e.target.value })} placeholder="Notification title" aria-label="Notification title" />;
    case "addTask":
      return (
        <input
          className="grow"
          value={step.title === "{previous}" ? "" : step.title}
          onChange={(e) => onChange({ ...step, title: e.target.value || "{previous}" })}
          placeholder="To-do (empty: the first line of the text so far)"
          aria-label="To-do title"
        />
      );
    case "saveFile":
      return <input className="grow" value={step.name} onChange={(e) => onChange({ ...step, name: e.target.value })} placeholder="File name" aria-label="File name" />;
    case "shortcut":
      return <input className="grow" value={step.name} onChange={(e) => onChange({ ...step, name: e.target.value })} placeholder="The shortcut's name, exactly as in Shortcuts" aria-label="Shortcut name" />;
    case "briefing":
      return <span className="grow muted small">Calendar, reminders, to-dos, weather and news.</span>;
  }
}

/** Build or edit an automation: when it runs, then its steps. */
function Builder({ start, onDone }: { start: Automation; onDone: (saved: boolean) => void }) {
  const [a, setA] = useState<Automation>(start);
  const [when, setWhen] = useState<When>(whenOf(start.trigger));
  const [words, setWords] = useState(whenOf(start.trigger) === "schedule" ? start.when : "");
  const [parsed, setParsed] = useState<[string, string] | null>(whenOf(start.trigger) === "schedule" ? [start.trigger, start.when] : null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (when !== "schedule") return;
    const t = setTimeout(() => {
      if (!words.trim()) return setParsed(null);
      api.automationTriggerParse(words).then((p) => setParsed(p && p[0] !== "launch" ? p : null), () => setParsed(null));
    }, 200);
    return () => clearTimeout(t);
  }, [words, when]);

  const trigger = when === "schedule" ? (parsed?.[0] ?? "") : when;
  const why = problem({ ...a, trigger });
  const setStep = (i: number, s: AutoStep) => setA({ ...a, steps: a.steps.map((x, j) => (j === i ? s : x)) });
  const save = async () => {
    setError(null);
    try {
      await api.automationSave({ ...a, trigger });
      onDone(true);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="auto-builder">
      <div className="auto-row">
        <input className="grow" value={a.name} onChange={(e) => setA({ ...a, name: e.target.value })} placeholder="Name, e.g. Morning AI news" aria-label="Automation name" />
        <select value={when} onChange={(e) => setWhen(e.target.value as When)} aria-label="When it runs">
          <option value="manual">When I run it</option>
          <option value="schedule">On a schedule</option>
          <option value="launch">When BYTE opens</option>
        </select>
      </div>
      {when === "schedule" && (
        <div className="auto-row">
          <input className="grow" value={words} onChange={(e) => setWords(e.target.value)} placeholder="When: every weekday at 8am" aria-label="When" />
          <span className="muted small">{words.trim() ? (parsed ? parsed[1] : "Try “every weekday at 8am” or “every monday at 9”.") : ""}</span>
        </div>
      )}
      <ol className="auto-steps">
        {a.steps.map((s, i) => (
          <li key={i}>
            <span className="step-no">{i + 1}</span>
            <select
              value={s.type}
              onChange={(e) => setStep(i, newStep(e.target.value as AutoStep["type"], a.name))}
              aria-label={`Step ${i + 1} kind`}
              title={STEP_KINDS.find((k) => k.type === s.type)?.hint}
            >
              {STEP_KINDS.map((k) => (
                <option key={k.type} value={k.type}>
                  {k.label}
                </option>
              ))}
            </select>
            <StepFields step={s} onChange={(n) => setStep(i, n)} />
            <button className="icon-btn sm" disabled={i === 0} onClick={() => setA({ ...a, steps: moveStep(a.steps, i, -1) })} aria-label={`Move step ${i + 1} up`}>
              <ArrowUp size={13} />
            </button>
            <button className="icon-btn sm" disabled={i === a.steps.length - 1} onClick={() => setA({ ...a, steps: moveStep(a.steps, i, 1) })} aria-label={`Move step ${i + 1} down`}>
              <ArrowDown size={13} />
            </button>
            <button className="icon-btn sm" disabled={a.steps.length === 1} onClick={() => setA({ ...a, steps: a.steps.filter((_, j) => j !== i) })} aria-label={`Remove step ${i + 1}`}>
              <X size={13} />
            </button>
          </li>
        ))}
      </ol>
      <p className="muted small">Each step gets the text from the step before: “summarize it” works on the answer above it.</p>
      {error && <div className="hint danger">{error}</div>}
      <div className="auto-row">
        <button className="btn sm ghost" disabled={a.steps.length >= 8} onClick={() => setA({ ...a, steps: [...a.steps, newStep("ask")] })}>
          <Plus size={13} /> Add a step
        </button>
        <span className="spacer" />
        {why && <span className="muted small">{why}</span>}
        <button className="btn sm ghost" onClick={() => onDone(false)}>
          Cancel
        </button>
        <button className="btn sm primary" disabled={!!why} onClick={() => void save()}>
          Save
        </button>
      </div>
    </div>
  );
}

/** Automations in the ✅ panel: a trigger and steps BYTE does one after another (Rust `automations.rs`). */
export function AutomationsSection({ onClose }: { onClose: () => void }) {
  const [list, setList] = useState<Automation[]>([]);
  const [editing, setEditing] = useState<Automation | null>(null);
  const [running, setRunning] = useState<Record<number, RunView>>({});
  const [made, setMade] = useState<{ id: number; result: ShortcutMade } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const selectChat = useStore((s) => s.selectChat);
  const reloadChats = useStore((s) => s.reloadChats);

  const load = useCallback(() => {
    api.automationsList().then(setList, (e) => setError(errorText(e)));
    setNow(Date.now());
  }, []);
  useEffect(load, [load]);
  useEffect(() => {
    const off = api.onAutomationProgress((v) => {
      setRunning((r) => ({ ...r, [v.automationId]: v }));
      if (v.finished) load();
    });
    return () => void off.then((f) => f());
  }, [load]);

  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    load();
  };
  const shortcut = (a: Automation) =>
    void guard(async () => {
      setMade({ id: a.id, result: await api.automationShortcut(a.id) });
    });
  const open = (chat: string) =>
    void guard(async () => {
      await reloadChats();
      await selectChat(chat);
      onClose();
    });

  return (
    <>
      <div className="recipe-box-head schedules-head">
        <Workflow size={16} />
        <h3>Automations</h3>
        <span className="spacer" />
        {!editing && (
          <button className="btn sm ghost" onClick={() => setEditing(blankAutomation())}>
            <Plus size={13} /> New
          </button>
        )}
      </div>
      <p className="muted small">
        Steps BYTE does one after another: when you run them, on a schedule, or when BYTE opens. Or just ask: “every weekday at 8am, find AI news, then summarize it, then save it to a file”.
      </p>
      {error && <div className="hint danger">{error}</div>}
      {editing && (
        <Builder
          key={editing.id}
          start={editing}
          onDone={() => {
            setEditing(null);
            load();
          }}
        />
      )}
      {list.length > 0 && (
        <ul className="schedule-list auto-list">
          {list.map((a) => {
            const live = running[a.id];
            const busy = live && !live.finished;
            return (
              <li key={a.id} className={a.enabled ? "" : "off"}>
                <Workflow size={15} />
                <div className="grow">
                  <b>{a.name}</b>
                  <div className="muted small">
                    {a.when}
                    {a.enabled && a.nextRun != null && ` · next ${dueText(a.nextRun, now)}`}
                    {busy && ` · ${runStatus(live.steps, false)}`}
                    {!busy && a.lastOk === false && <span className="hint danger"> · the last run stopped early</span>}
                  </div>
                  <div className="small prompt">{a.steps.map(stepLabel).join(" → ")}</div>
                  {made?.id === a.id &&
                    (made.result.opened ? (
                      <div className="small">Shortcuts is open: click “Add Shortcut”. Then it can go in the menu bar, or ask Siri to run “{a.name}”.</div>
                    ) : (
                      <div className="small auto-manual">
                        {made.result.message} In Shortcuts: new shortcut → add “Open URLs” → paste this link:
                        <code>{made.result.link}</code>
                        <button className="btn sm ghost" onClick={() => void navigator.clipboard.writeText(made.result.link)}>
                          Copy link
                        </button>
                      </div>
                    ))}
                </div>
                {a.lastChat && (
                  <button className="btn sm ghost" onClick={() => open(a.lastChat!)} title="Open the last run's result">
                    Open last
                  </button>
                )}
                <button className="btn sm ghost" disabled={busy} onClick={() => void guard(() => api.automationRun(a.id))} title="Run it now">
                  <Play size={13} /> {busy ? "Running…" : "Run now"}
                </button>
                <button className="icon-btn sm" onClick={() => shortcut(a)} title="Add to Shortcuts (menu bar, Siri)" aria-label={`Make a Shortcut for ${a.name}`}>
                  <Link2 size={13} />
                </button>
                <button className="icon-btn sm" onClick={() => setEditing(a)} title="Edit" aria-label={`Edit ${a.name}`}>
                  <Pencil size={13} />
                </button>
                <label className="switch" title={a.enabled ? "On" : "Off"}>
                  <input type="checkbox" checked={a.enabled} onChange={(e) => void guard(() => api.automationSave({ ...a, enabled: e.target.checked }))} aria-label={`${a.name} on`} />
                </label>
                <button className="icon-btn sm" onClick={() => void guard(() => api.automationDelete(a.id))} aria-label={`Delete ${a.name}`} title="Delete">
                  <Trash2 size={13} />
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}
