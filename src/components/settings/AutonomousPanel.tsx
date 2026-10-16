import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { FileText, Play, Plus, RotateCcw, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import {
  autoApi,
  canResume,
  DISCLAIMER,
  emptyContract,
  jobProblem,
  liftBudget,
  MESSAGE_OPTIONS,
  minutesText,
  splitList,
  STAGE_OPTIONS,
  STEP_OPTIONS,
  TIME_OPTIONS,
  type BudgetOption,
  type Contract,
  type Job,
  type StageDraft,
} from "../../lib/auto";
import { isWindows } from "../../lib/keys";
import type { Settings } from "../../lib/types";
import { useStore } from "../../state/store";

function BudgetSelect({ label, value, options, onChange }: { label: string; value: string; options: BudgetOption[]; onChange: (v: string) => void }) {
  return (
    <label className="column" style={{ gap: 2 }}>
      <small>{label}</small>
      <select className="text-input" value={value} aria-label={label} onChange={(e) => onChange(e.target.value)}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Settings, Control: autonomous mode. A job is a list of stages, run one after another under a contract you set first. */
export function AutonomousPanel({ settings }: { settings: Settings | null }) {
  const update = useStore((s) => s.updateSettings);
  const reloadChats = useStore((s) => s.reloadChats);
  const selectChat = useStore((s) => s.selectChat);
  const [read, setRead] = useState(false);
  const [request, setRequest] = useState("");
  const [stages, setStages] = useState<StageDraft[]>([]);
  const [contract, setContract] = useState<Contract>(emptyContract());
  const [appsText, setAppsText] = useState("");
  const [peopleText, setPeopleText] = useState("");
  const [jobs, setJobs] = useState<Job[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [keyNote, setKeyNote] = useState<string | null>(null);
  const [keyDraft, setKeyDraft] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const on = settings?.autoMode ?? false;
  const accepted = settings?.autoAccepted ?? false;
  const control = settings?.pcControl ?? false;

  const refresh = useCallback(() => {
    if (!inTauri) return;
    autoApi.jobs().then(setJobs, () => undefined);
    autoApi.status().then((s) => setRunning(s.running), () => undefined);
  }, []);
  useEffect(() => {
    refresh();
    if (!inTauri) return;
    autoApi.contract().then((c) => {
      setContract(c);
      setAppsText(c.apps.join(", "));
      setPeopleText(c.mayMessage.join(", "));
    }, () => undefined);
    const off = listen("autonomy://event", () => refresh());
    return () => void off.then((f) => f());
  }, [refresh, on]);

  if (!isWindows()) return null;

  const guard = async (f: () => Promise<unknown>) => {
    setError(null);
    setBusy(true);
    try {
      await f();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const setC = (patch: Partial<Contract>) => setContract((c) => ({ ...c, ...patch }));
  const effective: Contract = { ...contract, apps: splitList(appsText), mayMessage: splitList(peopleText) };
  const problem = jobProblem(request, stages, effective);
  const addFolder = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string" && !contract.folders.includes(picked)) setC({ folders: [...contract.folders, picked] });
  };

  return (
    <div className="section" id="autonomous-mode">
      <h4>Autonomous mode</h4>
      {error && <small className="error">{error}</small>}
      <label className="field">
        <span>
          Let BYTE work through jobs by itself
          <small>You give it a list of stages and a contract (which programs, folders and people it may use, and how long). It does the stages one after another and writes you a report. Off until you read the notice below and accept it.</small>
        </span>
        <input
          type="checkbox"
          checked={on}
          disabled={!control && !on}
          aria-label="Autonomous mode"
          onChange={(e) => {
            if (e.target.checked && !accepted) setRead(true);
            else void update({ autoMode: e.target.checked });
          }}
        />
      </label>
      {!control && <small>Turn on PC control first.</small>}
      {read && !accepted && (
        <div className="callout" role="region" aria-label="Autonomous mode notice" style={{ margin: "8px 0" }}>
          {DISCLAIMER.map((d) => (
            <p key={d}>{d}</p>
          ))}
          <label className="row" style={{ gap: 8 }}>
            <input type="checkbox" checked={read} readOnly aria-label="I have read this" />
            <span>I have read this and I accept it.</span>
          </label>
          <div className="row" style={{ gap: 8, marginTop: 8 }}>
            <button className="btn sm primary" onClick={() => { void update({ autoAccepted: true, autoMode: true }); setRead(false); }}>
              Accept and turn on
            </button>
            <button className="btn sm ghost" onClick={() => setRead(false)}>Cancel</button>
          </div>
        </div>
      )}
      {on && accepted && (
        <>
          <div className="field">
            <span>
              Stop key
              <small>Works from anywhere while a job runs. {keyNote ?? "F8, F9 and Esc are used by guided help."}</small>
            </span>
            <input
              className="text-input"
              value={keyDraft ?? settings?.autoStopKey ?? "Ctrl+Alt+F12"}
              aria-label="Stop key"
              onChange={(e) => setKeyDraft(e.target.value)}
              onBlur={() => {
                if (keyDraft === null) return;
                autoApi.stopKeyCheck(keyDraft).then(
                  () => { setKeyNote("Saved."); void update({ autoStopKey: keyDraft }); setKeyDraft(null); },
                  (e) => setKeyNote(errorText(e)),
                );
              }}
            />
          </div>
          <label className="field">
            <span>
              Pause when I use the PC
              <small>If you move the mouse or press keys while a job runs, it pauses until you press Resume. This is a best effort and can be fooled by programs that send input.</small>
            </span>
            <input type="checkbox" checked={settings?.autoPauseOnInput ?? false} onChange={(e) => void update({ autoPauseOnInput: e.target.checked })} aria-label="Pause when I use the PC" />
          </label>
          <label className="field">
            <span>
              Keep the PC awake while a job runs
              <small>The PC and the screen stay on until the job ends, then sleep as usual.</small>
            </span>
            <input type="checkbox" checked={settings?.autoKeepAwake ?? true} onChange={(e) => void update({ autoKeepAwake: e.target.checked })} aria-label="Keep the PC awake" />
          </label>
          <label className="field">
            <span>
              Tell me when it needs me or ends
              <small>A notification when a job waits for your answer, pauses or finishes.</small>
            </span>
            <input type="checkbox" checked={settings?.autoNotify ?? true} onChange={(e) => void update({ autoNotify: e.target.checked })} aria-label="Notifications for jobs" />
          </label>
          <label className="field">
            <span>
              Read the report aloud
              <small>When a job ends BYTE says a short version. The full report is always saved as a chat.</small>
            </span>
            <input type="checkbox" checked={settings?.autoSpeakReport ?? false} onChange={(e) => void update({ autoSpeakReport: e.target.checked })} aria-label="Read the report aloud" />
          </label>
          <div className="field">
            <span>
              Most stages in one job
              <small>{settings?.autoMaxStages === "unlimited" ? "No limit on how many stages a job can have." : "A longer list is refused until you raise this."}</small>
            </span>
            <select value={settings?.autoMaxStages ?? "10"} aria-label="Most stages in one job" onChange={(e) => void update({ autoMaxStages: e.target.value as "10" | "25" | "50" | "unlimited" })}>
              {STAGE_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>{o.label}</option>
              ))}
            </select>
          </div>

          <div className="field" style={{ display: "block" }}>
            <span>
              New job
              <small>Say what you want done, in your own words. BYTE splits it into stages and shows you the list before anything starts. A numbered list is used as it is.</small>
            </span>
            <textarea className="text-input" rows={4} value={request} aria-label="The job" placeholder="Open the sales sheet, copy this week's numbers into a new note, and save it in Work." onChange={(e) => { setRequest(e.target.value); setStages([]); }} />
            <div className="row" style={{ gap: 8, margin: "8px 0" }}>
              <button className="btn sm" disabled={busy || !request.trim() || running} onClick={() => void guard(async () => setStages(await autoApi.split(request)))}>
                Split into stages
              </button>
            </div>
            {stages.length > 0 && (
              <ol aria-label="Stages" style={{ paddingLeft: 20 }}>
                {stages.map((s, i) => (
                  <li key={i} className="row" style={{ gap: 6, marginBottom: 4 }}>
                    <input className="text-input grow" value={s.text} aria-label={`Stage ${i + 1}`} onChange={(e) => setStages(stages.map((x, j) => (j === i ? { ...x, text: e.target.value } : x)))} />
                    <small>{s.tool === "auto" ? "" : s.tool}</small>
                    <button className="icon-btn" aria-label={`Remove stage ${i + 1}`} onClick={() => setStages(stages.filter((_, j) => j !== i))}>
                      <X size={14} />
                    </button>
                  </li>
                ))}
              </ol>
            )}
            {stages.length > 0 && (
              <>
                <h5>Run contract</h5>
                <small>Anything outside this pauses the job and waits for you.</small>
                <label className="column">
                  <span>Programs it may use (use “browser” for BYTE's browser)</span>
                  <input className="text-input" value={appsText} placeholder="notepad, excel, browser" aria-label="Programs it may use" onChange={(e) => setAppsText(e.target.value)} />
                </label>
                <div className="column">
                  <span>Folders it may use</span>
                  {contract.folders.map((f) => (
                    <div key={f} className="row" style={{ gap: 6 }}>
                      <span className="grow">{f}</span>
                      <button className="icon-btn" aria-label={`Remove ${f}`} onClick={() => setC({ folders: contract.folders.filter((x) => x !== f) })}>
                        <Trash2 size={14} />
                      </button>
                    </div>
                  ))}
                  <button className="btn sm" onClick={() => void addFolder()}>
                    <Plus size={14} /> Add a folder
                  </button>
                </div>
                <label className="column">
                  <span>People it may message (names as your connectors know them; none if empty)</span>
                  <input className="text-input" value={peopleText} placeholder="Mom, Alex" aria-label="People it may message" onChange={(e) => setPeopleText(e.target.value)} />
                </label>
                <label className="row" style={{ gap: 8 }}>
                  <input type="checkbox" checked={contract.neverDelete} onChange={(e) => setC({ neverDelete: e.target.checked })} aria-label="Never delete" />
                  <span>Never delete anything</span>
                </label>
                <label className="row" style={{ gap: 8 }}>
                  <input type="checkbox" checked disabled aria-label="Never spend money" />
                  <span>Never spend money (always on)</span>
                </label>
                <div className="row" style={{ gap: 12, flexWrap: "wrap" }}>
                  <BudgetSelect label="Time" value={contract.timeBudget} options={TIME_OPTIONS} onChange={(v) => setC({ timeBudget: v })} />
                  <BudgetSelect label="Steps" value={contract.stepBudget} options={STEP_OPTIONS} onChange={(v) => setC({ stepBudget: v })} />
                  <BudgetSelect label="Messages" value={contract.messageBudget} options={MESSAGE_OPTIONS} onChange={(v) => setC({ messageBudget: v })} />
                </div>
                {problem && <small className="error">{problem}</small>}
                <div className="row" style={{ gap: 8, marginTop: 8 }}>
                  <button
                    className="btn sm primary"
                    disabled={busy || !!problem || running}
                    onClick={() => void guard(async () => { await autoApi.start(request, stages, effective); setStages([]); setRequest(""); refresh(); })}
                  >
                    <Play size={14} /> Yes, start the job
                  </button>
                </div>
              </>
            )}
          </div>

          <div className="field" style={{ display: "block" }}>
            <span>
              Jobs
              <small>Each stage is saved when it ends. If BYTE closes, a job waits here and picks up at the first stage that is not finished.</small>
            </span>
            {jobs.length === 0 ? (
              <small>None yet.</small>
            ) : (
              <ul className="plain-list" aria-label="Jobs" style={{ margin: "8px 0" }}>
                {jobs.slice(0, 12).map((j) => {
                  const lifted = liftBudget(j.contract, j.stopReason);
                  return (
                    <li key={j.id} className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                      <span className="grow">
                        {j.title}
                        <small> · {j.state} · {j.stages.filter((s) => s.status === "done" || s.status === "skipped").length} of {j.stages.length} stages · {minutesText(j.usedMs)}{j.stopReason ? ` · ${j.stopReason}` : ""}</small>
                      </span>
                      {j.chatId && (
                        <button className="btn sm ghost" onClick={() => void reloadChats().then(() => selectChat(j.chatId))}>
                          <FileText size={14} /> Report
                        </button>
                      )}
                      {canResume(j) && (
                        <button className="btn sm" disabled={busy || running} onClick={() => void guard(async () => { await autoApi.resume(j.id); refresh(); })}>
                          <RotateCcw size={14} /> Resume
                        </button>
                      )}
                      {canResume(j) && lifted && (
                        <button className="btn sm" disabled={busy || running} onClick={() => void guard(async () => { await autoApi.resume(j.id, lifted); refresh(); })}>
                          Resume with no limit on that
                        </button>
                      )}
                      <button className="icon-btn" aria-label={`Delete ${j.title}`} disabled={running && j.state === "running"} onClick={() => void autoApi.jobDelete(j.id).then(refresh)}>
                        <Trash2 size={14} />
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </>
      )}
    </div>
  );
}
