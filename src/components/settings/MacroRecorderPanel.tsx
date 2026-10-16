import { listen } from "@tauri-apps/api/event";
import { Circle, Square } from "lucide-react";
import { useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { autoApi, type Recording } from "../../lib/auto";
import { controlApi, routineProblem, stepLabel } from "../../lib/control";
import { isWindows } from "../../lib/keys";
import type { Settings } from "../../lib/types";
import { useStore } from "../../state/store";

/** Settings, Control: "Watch me do this once". BYTE notes the buttons you press, never what you type, and offers a routine. */
export function MacroRecorderPanel({ settings }: { settings: Settings | null }) {
  const update = useStore((s) => s.updateSettings);
  const [active, setActive] = useState(false);
  const [rec, setRec] = useState<Recording | null>(null);
  const [phrase, setPhrase] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const on = settings?.macroRecorder ?? false;
  const control = settings?.pcControl ?? false;

  useEffect(() => {
    if (!inTauri) return;
    autoApi.macroActive().then(setActive, () => undefined);
    const off = listen<Recording>("macro://done", (e) => {
      setActive(false);
      setRec(e.payload);
    });
    return () => void off.then((f) => f());
  }, []);

  if (!isWindows()) return null;
  const guard = async (f: () => Promise<unknown>) => {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const draft = rec ? { id: "", phrase, steps: rec.steps, schedule: "", enabled: true } : null;
  const problem = draft ? routineProblem(draft) : null;

  return (
    <div className="section" id="macro-recorder">
      <h4>Watch me do this once</h4>
      {error && <small className="error">{error}</small>}
      <label className="field">
        <span>
          Let BYTE learn a routine by watching
          <small>
            While you record, a small badge stays on screen. BYTE notes the buttons you press (by their names, not pictures) and the key combinations you use. It never keeps what you type, never notes
            password or card fields, and ignores programs on your never-touch list. Then you can name it, edit it and replay it. Off until you turn it on.
          </small>
        </span>
        <input type="checkbox" checked={on} disabled={!control && !on} onChange={(e) => void update({ macroRecorder: e.target.checked })} aria-label="Watch me do this once" />
      </label>
      {on && control && (
        <div className="row" style={{ gap: 8, margin: "8px 0" }}>
          {!active ? (
            <button className="btn sm" onClick={() => void guard(async () => { setRec(null); setSaved(false); await autoApi.macroStart(); setActive(true); })}>
              <Circle size={14} /> Start watching
            </button>
          ) : (
            <button className="btn sm primary" onClick={() => void guard(async () => { setRec(await autoApi.macroStop()); setActive(false); })}>
              <Square size={14} /> Finish recording
            </button>
          )}
          {active && <small>Recording. Do the task once in your program, then finish here or on the badge.</small>}
        </div>
      )}
      {rec && draft && (
        <div className="column" style={{ gap: 6 }}>
          {rec.steps.length === 0 ? <small>BYTE did not note any steps.</small> : (
            <ol aria-label="Recorded steps" style={{ paddingLeft: 20 }}>
              {rec.steps.map((s, i) => (
                <li key={i}>{stepLabel(s)}</li>
              ))}
            </ol>
          )}
          {rec.typedParts > 0 && <small>{rec.typedParts} typed part(s) were left out on purpose. Add a “Type some text” step in the routine if you want one.</small>}
          {rec.secretSkipped > 0 && <small>{rec.secretSkipped} password or card field(s) were skipped.</small>}
          {rec.cut > 0 && <small>{rec.cut} more step(s) did not fit: a routine holds 12.</small>}
          {saved ? (
            <small>Saved. Find it under Voice routines, where you can edit it.</small>
          ) : (
            rec.steps.length > 0 && (
              <>
                <label className="column">
                  <span>What you say to run it</span>
                  <input className="text-input" value={phrase} placeholder="close the day" maxLength={60} aria-label="Phrase for the recorded routine" onChange={(e) => setPhrase(e.target.value)} />
                </label>
                {problem && phrase.trim() && <small className="error">{problem}</small>}
                <div className="row" style={{ gap: 8 }}>
                  <button className="btn sm primary" disabled={!!problem} onClick={() => void guard(async () => { await controlApi.routineSave(draft); setSaved(true); })}>
                    Save as a routine
                  </button>
                  <button className="btn sm ghost" onClick={() => setRec(null)}>Throw away</button>
                </div>
              </>
            )
          )}
        </div>
      )}
    </div>
  );
}
