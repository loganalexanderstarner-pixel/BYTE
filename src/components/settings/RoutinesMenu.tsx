import { Pencil, Play, Plus, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { controlApi, newStep, routineProblem, STEP_KINDS, stepLabel, type Routine, type RoutineStep } from "../../lib/control";
import type { Settings } from "../../lib/types";
import { useStore } from "../../state/store";

const BLANK: Routine = { id: "", phrase: "", steps: [], schedule: "", enabled: true };

/** Settings → PC control → Voice routines: a phrase you say (or a schedule) that runs a list of PC steps. */
export function RoutinesMenu({ settings }: { settings: Settings | null }) {
  const update = useStore((s) => s.updateSettings);
  const [list, setList] = useState<Routine[]>([]);
  const [editing, setEditing] = useState<Routine | null>(null);
  const [when, setWhen] = useState("");
  const [whenNote, setWhenNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const on = settings?.routinesEnabled ?? false;

  const load = useCallback(() => {
    if (inTauri) controlApi.routines().then(setList, (e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load]);

  const setStep = (i: number, s: RoutineStep) => editing && setEditing({ ...editing, steps: editing.steps.map((x, j) => (j === i ? s : x)) });

  const readWhen = async () => {
    if (!editing) return;
    if (!when.trim()) {
      setEditing({ ...editing, schedule: "" });
      setWhenNote(null);
      return;
    }
    const r = await controlApi.scheduleParse(when);
    if (r) {
      setEditing({ ...editing, schedule: r[0] });
      setWhenNote(r[1]);
    } else {
      setWhenNote("I couldn't read that. Try “every weekday at 7am”.");
    }
  };

  const save = async () => {
    if (!editing) return;
    const problem = routineProblem(editing);
    if (problem) {
      setError(problem);
      return;
    }
    setError(null);
    try {
      await controlApi.routineSave(editing);
      setEditing(null);
      setWhen("");
      setWhenNote(null);
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="field" style={{ display: "block" }}>
      <label className="row" style={{ alignItems: "flex-start", gap: 12 }}>
        <span className="grow">
          Voice routines
          <small>A phrase like “game time” that does several things at once: open a program, set the volume, close another, turn notifications off, open a web address. Say it after “Hey BYTE”, or give it a schedule.</small>
        </span>
        <input type="checkbox" checked={on} onChange={(e) => void update({ routinesEnabled: e.target.checked })} aria-label="Voice routines" />
      </label>
      {error && <small className="error">{error}</small>}
      {said && <small>{said}</small>}
      {on && !editing && (
        <>
          <ul className="plain-list" style={{ margin: "8px 0" }}>
            {list.length === 0 && <li><small>No routines yet.</small></li>}
            {list.map((r) => (
              <li key={r.id} className="row" style={{ gap: 8, alignItems: "center" }}>
                <span className="grow">
                  <strong>{r.phrase}</strong>
                  <small>
                    {r.steps.map(stepLabel).join(" · ")}
                    {r.schedule ? ` · ${r.schedule}` : ""}
                  </small>
                </span>
                <button className="btn sm" aria-label={`Run ${r.phrase}`} onClick={() => void controlApi.routineRun(r.id).then(setSaid, (e) => setError(errorText(e)))}>
                  <Play size={14} /> Run
                </button>
                <button className="icon-btn" aria-label={`Edit ${r.phrase}`} onClick={() => { setEditing(r); setWhen(""); setWhenNote(r.schedule || null); }}>
                  <Pencil size={14} />
                </button>
                <button className="icon-btn" aria-label={`Remove ${r.phrase}`} onClick={() => void controlApi.routineDelete(r.id).then(load, (e) => setError(errorText(e)))}>
                  <Trash2 size={14} />
                </button>
              </li>
            ))}
          </ul>
          <button className="btn sm" onClick={() => { setEditing({ ...BLANK }); setWhen(""); setWhenNote(null); }}>
            <Plus size={14} /> New routine
          </button>
        </>
      )}
      {on && editing && (
        <div className="column" style={{ gap: 8, marginTop: 8 }}>
          <label className="column">
            <span>What you say</span>
            <input className="text-input" value={editing.phrase} placeholder="game time" maxLength={60} onChange={(e) => setEditing({ ...editing, phrase: e.target.value })} />
          </label>
          <div className="column" style={{ gap: 6 }}>
            <span>Steps, in order</span>
            {editing.steps.map((s, i) => (
              <div key={i} className="row" style={{ gap: 6, alignItems: "center" }}>
                <small>{i + 1}.</small>
                <select
                  className="text-input"
                  value={s.type}
                  aria-label={`Step ${i + 1} kind`}
                  onChange={(e) => setStep(i, newStep(e.target.value as RoutineStep["type"]))}
                >
                  {STEP_KINDS.map((k) => (
                    <option key={k.type} value={k.type}>{k.label}</option>
                  ))}
                </select>
                {(s.type === "openApp" || s.type === "closeApp" || s.type === "runAutomation") && (
                  <input className="text-input grow" value={s.name} placeholder={s.type === "runAutomation" ? "Automation name" : "Program name"} aria-label={`Step ${i + 1} name`} onChange={(e) => setStep(i, { ...s, name: e.target.value })} />
                )}
                {s.type === "volume" && (
                  <input className="text-input" type="number" min={0} max={100} value={s.percent} aria-label={`Step ${i + 1} volume`} onChange={(e) => setStep(i, { ...s, percent: Math.max(0, Math.min(100, Number(e.target.value) || 0)) })} />
                )}
                {s.type === "doNotDisturb" && (
                  <select className="text-input" value={s.on ? "off" : "on"} aria-label={`Step ${i + 1} notifications`} onChange={(e) => setStep(i, { ...s, on: e.target.value === "off" })}>
                    <option value="off">Turn notifications off (back on at the next start or after 12 hours)</option>
                    <option value="on">Turn notifications on</option>
                  </select>
                )}
                {s.type === "openUrl" && (
                  <input className="text-input grow" value={s.url} placeholder="https://" aria-label={`Step ${i + 1} address`} onChange={(e) => setStep(i, { ...s, url: e.target.value })} />
                )}
                {/* --- track:autonomy begin --- */}
                {s.type === "clickElement" && (
                  <>
                    <input className="text-input grow" value={s.window} placeholder="Window title" aria-label={`Step ${i + 1} window`} onChange={(e) => setStep(i, { ...s, window: e.target.value })} />
                    <input className="text-input grow" value={s.name} placeholder="Button name" aria-label={`Step ${i + 1} button`} onChange={(e) => setStep(i, { ...s, name: e.target.value })} />
                  </>
                )}
                {s.type === "pressKeys" && (
                  <input className="text-input grow" value={s.combo} placeholder="ctrl+s" aria-label={`Step ${i + 1} keys`} onChange={(e) => setStep(i, { ...s, combo: e.target.value })} />
                )}
                {s.type === "typeText" && (
                  <input className="text-input grow" value={s.text} placeholder="Text to type" aria-label={`Step ${i + 1} text`} onChange={(e) => setStep(i, { ...s, text: e.target.value })} />
                )}
                {/* --- track:autonomy end --- */}
                <button className="icon-btn" aria-label={`Remove step ${i + 1}`} onClick={() => setEditing({ ...editing, steps: editing.steps.filter((_, j) => j !== i) })}>
                  <X size={14} />
                </button>
              </div>
            ))}
            <button className="btn sm" disabled={editing.steps.length >= 12} onClick={() => setEditing({ ...editing, steps: [...editing.steps, newStep("openApp")] })}>
              <Plus size={14} /> Add a step
            </button>
          </div>
          <label className="column">
            <span>When (optional)</span>
            <input className="text-input" value={when} placeholder="every weekday at 7am" onChange={(e) => setWhen(e.target.value)} onBlur={() => void readWhen()} />
            <small>{whenNote ?? "Leave empty to run only when you say it."}</small>
          </label>
          <div className="row" style={{ gap: 8 }}>
            <button className="btn sm primary" onClick={() => void save()}>Save routine</button>
            <button className="btn sm ghost" onClick={() => { setEditing(null); setError(null); }}>Cancel</button>
          </div>
        </div>
      )}
    </div>
  );
}
