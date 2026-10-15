import { ask } from "@tauri-apps/plugin-dialog";
import { FlaskConical, Plus, Square, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { controlApi, noteLine, planApi, recommendedFirst, type AppNote, type CapableModel } from "../../lib/control";
import { isWindows } from "../../lib/keys";
import type { Settings } from "../../lib/types";
import { useStore } from "../../state/store";
import { scoreWords, useSelfTest } from "../models/PcControlModels";

/** What cloud planning sends, said exactly (also the text of the one-time confirmation). */
export const CLOUD_PLANNING_SENDS =
  "When BYTE plans a PC job with BYTE Cloud, it sends your request and the names of the buttons and boxes in the window in front. It does not send a picture, typed text from the window, or password and card fields, and it never does this for private chats, kids mode, BYTE's own window, or programs you excluded.";

/** Settings → Control and screen → Planning: where plans are made, notes, dry run and the model test. Windows only. */
export function PlanningSettings({ settings }: { settings: Settings | null }) {
  const update = useStore((s) => s.updateSettings);
  const [models, setModels] = useState<CapableModel[]>([]);
  const [notes, setNotes] = useState<AppNote[]>([]);
  const [draft, setDraft] = useState({ program: "", goal: "", text: "" });
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(() => {
    if (!inTauri) return;
    controlApi.status().then((s) => setModels(recommendedFirst(s.models).filter((m) => m.installedKey)), () => undefined);
    planApi.notes().then(setNotes, () => undefined);
  }, []);
  useEffect(refresh, [refresh]);
  const test = useSelfTest(refresh);
  if (!isWindows() || !(settings?.pcControl || settings?.guidedHelp)) return null;

  const guard = async (f: () => Promise<unknown>) => {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const where = settings.controlPlanWhere ?? "local";
  const cloudOn = settings.controlCloudPlanning ?? false;
  const notesOn = settings.controlAppNotes ?? false;

  const toggleCloud = async (on: boolean) => {
    if (on && !settings.controlCloudConfirmed) {
      const yes = await ask(`${CLOUD_PLANNING_SENDS}\n\nTurn on cloud planning?`, { title: "Plan with BYTE Cloud", kind: "warning" });
      if (!yes) return;
      await update({ controlCloudPlanning: true, controlCloudConfirmed: true });
      return;
    }
    await update({ controlCloudPlanning: on });
  };

  return (
    <div className="section" id="planning">
      <h4>Planning</h4>
      {error && <small className="error">{error}</small>}
      {test.error && <small className="error">{test.error}</small>}

      <label className="field">
        <span>
          Plan with BYTE Cloud
          <small>{CLOUD_PLANNING_SENDS} Off until you turn it on; while off, every plan is made on this PC.</small>
        </span>
        <input type="checkbox" checked={cloudOn} onChange={(e) => void guard(() => toggleCloud(e.target.checked))} aria-label="Plan with BYTE Cloud" />
      </label>
      <div className="field">
        <span>
          Ask where
          <small>
            Local: this PC's model plans (the default). Cloud: BYTE Cloud plans. Auto: this PC's model, unless it is too small to plan reliably and cloud planning is on. Cloud and Auto do
            nothing until “Plan with BYTE Cloud” is on. Used for PC control and for guided help.
          </small>
        </span>
        <select value={where} aria-label="Ask where" onChange={(e) => void update({ controlPlanWhere: e.target.value as "local" | "cloud" | "auto" })}>
          <option value="local">Local</option>
          <option value="cloud" disabled={!cloudOn}>
            Cloud
          </option>
          <option value="auto" disabled={!cloudOn}>
            Auto
          </option>
        </select>
      </div>

      <label className="field">
        <span>
          Dry run
          <small>BYTE lists what it would click, type and press, and does none of it. You can also say “dry run: open Notepad and type hello” in the chat.</small>
        </span>
        <input type="checkbox" checked={settings.controlDryRun ?? false} onChange={(e) => void update({ controlDryRun: e.target.checked })} aria-label="Dry run" />
      </label>

      <label className="field">
        <span>
          Notes about programs
          <small>
            After a job works, BYTE remembers which buttons did it (their names only, never what you typed) and uses that as a hint next time. Kept on this PC only. Off until you turn it on.
          </small>
        </span>
        <input type="checkbox" checked={notesOn} onChange={(e) => void update({ controlAppNotes: e.target.checked })} aria-label="Notes about programs" />
      </label>
      {notesOn && (
        <>
          <label className="field">
            <span>
              Learn from corrections
              <small>
                When you stop a job and carry on in another program, BYTE offers to save a note about it. It asks every time and never saves on its own. While it waits it checks which program is in
                front every couple of seconds for up to two minutes.
              </small>
            </span>
            <input type="checkbox" checked={settings.controlLearn ?? false} onChange={(e) => void update({ controlLearn: e.target.checked })} aria-label="Learn from corrections" />
          </label>
          <div className="field" style={{ display: "block" }}>
            <span>
              Saved notes ({notes.length})<small>These are used as hints. Delete any you do not want.</small>
            </span>
            <div className="column" style={{ gap: 6, margin: "8px 0" }}>
              {notes.map((n) => (
                <div key={n.id} className="row" style={{ gap: 8, alignItems: "center" }}>
                  <span className="grow">
                    <strong>{n.app}</strong> <small>{noteLine(n)}</small>
                  </span>
                  <button className="icon-btn" aria-label={`Delete the note for ${n.app}`} onClick={() => void guard(async () => setNotes(await planApi.noteDelete(n.id)))}>
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
            </div>
            <form
              className="row"
              style={{ gap: 6, flexWrap: "wrap" }}
              onSubmit={(e) => {
                e.preventDefault();
                void guard(async () => {
                  setNotes(await planApi.noteAdd(draft.program, draft.goal, draft.text));
                  setDraft({ program: "", goal: "", text: "" });
                });
              }}
            >
              <input value={draft.program} onChange={(e) => setDraft({ ...draft, program: e.target.value })} placeholder="Program (notepad)" aria-label="Program" style={{ width: 130 }} />
              <input value={draft.goal} onChange={(e) => setDraft({ ...draft, goal: e.target.value })} placeholder="When asked (change the font)" aria-label="When asked" style={{ width: 190 }} />
              <input value={draft.text} onChange={(e) => setDraft({ ...draft, text: e.target.value })} placeholder="What to know (the font is under the gear)" aria-label="What to know" className="grow" />
              <button className="btn sm" type="submit">
                <Plus size={14} /> Add note
              </button>
              {notes.length > 0 && (
                <button className="btn sm" type="button" onClick={() => void guard(async () => { await planApi.notesClear(); setNotes([]); })}>
                  <Trash2 size={14} /> Clear all
                </button>
              )}
            </form>
          </div>
        </>
      )}

      <div className="field" style={{ display: "block" }}>
        <span>
          Test a model
          <small>Runs 40 standard tasks (open a program, switch a window, type, click a named button, change a setting, find a file, several steps, and jobs that should be refused). Nothing is clicked or typed.</small>
        </span>
        <div className="column" style={{ gap: 8, marginTop: 8 }}>
          {models.length === 0 && <small>Download a model for PC control first (Models and speed → PC control).</small>}
          {models.map((m) => {
            const testing = test.running === m.installedKey;
            return (
              <div key={m.id} className="row" style={{ gap: 8, alignItems: "center" }}>
                <span className="grow">
                  {m.name}
                  <small style={{ display: "block" }}>
                    {scoreWords(m.score)}
                    {testing && test.progress ? ` · testing ${test.progress.done} of ${test.progress.total}…` : ""}
                  </small>
                </span>
                {testing ? (
                  <button className="btn sm" onClick={test.stop}>
                    <Square size={14} /> Stop
                  </button>
                ) : (
                  <button className="btn sm" disabled={!!test.running} onClick={() => void test.start(m.installedKey!)}>
                    <FlaskConical size={14} /> Test
                  </button>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
