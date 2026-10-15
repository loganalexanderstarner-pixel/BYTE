import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { Download, Plus, Square, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { addApp, controlApi, sizeText, type ControlStatus, type LogEntry } from "../../lib/control";
import { isWindows } from "../../lib/keys";
import type { Settings } from "../../lib/types";
import { useStore } from "../../state/store";
import { RoutinesMenu } from "./RoutinesMenu";

/** Settings → Features → PC control (Windows): what BYTE may do on this PC by voice, with which model, and what it did. */
export function ControlSettings({ settings }: { settings: Settings | null }) {
  const update = useStore((s) => s.updateSettings);
  const downloads = useStore((s) => s.downloads);
  const [status, setStatus] = useState<ControlStatus | null>(null);
  const [log, setLog] = useState<LogEntry[]>([]);
  const [running, setRunning] = useState<string[]>([]);
  const [appDraft, setAppDraft] = useState("");
  const [folderError, setFolderError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const on = settings?.pcControl ?? false;

  const refresh = useCallback(() => {
    if (!inTauri) return;
    controlApi.status().then(setStatus, (e) => setError(errorText(e)));
    controlApi.log(60).then(setLog, () => undefined);
  }, []);
  useEffect(() => {
    refresh();
  }, [refresh, on]);
  // Follow a run while it happens (the state event) and when a download finishes.
  useEffect(() => {
    if (!inTauri) return;
    const off = listen<{ active: boolean }>("control://state", () => refresh());
    return () => void off.then((f) => f());
  }, [refresh]);
  const finished = Object.values(downloads).filter((d) => d?.phase === "finished").length;
  useEffect(() => {
    refresh();
  }, [finished, refresh]);

  if (!isWindows()) return null;

  const allowed = settings?.controlAllowedApps ?? [];
  const folders = settings?.findFolders ?? [];
  const guard = async (f: () => Promise<unknown>) => {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const addFolder = async () => {
    setFolderError(null);
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    try {
      const ok = await controlApi.checkFolder(picked);
      if (!folders.includes(ok)) await update({ findFolders: [...folders, ok] });
    } catch (e) {
      setFolderError(errorText(e));
    }
  };

  return (
    <div className="section" id="pc-control">
      <h4>PC control</h4>
      {error && <small className="error">{error}</small>}
      <label className="field">
        <span>
          Let BYTE use this PC for you
          <small>
            Say “Hey BYTE, open Notepad”, “switch to Chrome”, “lock the computer”, or ask for something bigger and BYTE reads the window in front as a list of
            buttons (not a picture), plans a few steps, does them, and checks each one worked. It asks first before shutting down, closing something, deleting,
            or anything that sends or buys. It never runs in private chats. Off until you turn it on.
          </small>
        </span>
        <input type="checkbox" checked={on} onChange={(e) => void update({ pcControl: e.target.checked })} aria-label="Let BYTE use this PC for you" />
      </label>
      {status?.active && (
        <div className="field">
          <span>
            BYTE is using your PC now.
            <small>It stops before its next step when you press Stop (or say “stop”).</small>
          </span>
          <button className="btn sm primary" onClick={() => void controlApi.stop()}>
            <Square size={14} /> Stop
          </button>
        </div>
      )}
      {on && (
        <>
          <div className="field">
            <span>
              How long one run may take
              <small>
                {settings?.controlRunLimit === "unlimited"
                  ? "No time limit. BYTE keeps going until the job is done or you press Stop (or say “stop”). Risky steps still ask first, and a plan is still limited to 12 steps at a time."
                  : "BYTE stops by itself when a run reaches this time. Pick a longer time for big jobs and routines."}
              </small>
            </span>
            <select
              value={settings?.controlRunLimit ?? "90s"}
              aria-label="How long one PC-control run may take"
              onChange={(e) => void update({ controlRunLimit: e.target.value as "90s" | "3m" | "10m" | "30m" | "1h" | "unlimited" })}
            >
              <option value="90s">90 seconds</option>
              <option value="3m">3 minutes</option>
              <option value="10m">10 minutes</option>
              <option value="30m">30 minutes</option>
              <option value="1h">1 hour</option>
              <option value="unlimited">Unlimited</option>
            </select>
          </div>
          <div className="field">
            <span>
              Steps in one plan
              <small>
                {settings?.controlMaxSteps === "unlimited"
                  ? "No limit on how many steps BYTE plans at once. Risky steps still ask first and Stop always works."
                  : "The most steps BYTE will plan at once. Longer jobs get planned again as they go."}
              </small>
            </span>
            <select
              value={settings?.controlMaxSteps ?? "12"}
              aria-label="Steps in one plan"
              onChange={(e) => void update({ controlMaxSteps: e.target.value as "12" | "25" | "50" | "100" | "unlimited" })}
            >
              <option value="12">12 steps</option>
              <option value="25">25 steps</option>
              <option value="50">50 steps</option>
              <option value="100">100 steps</option>
              <option value="unlimited">Unlimited</option>
            </select>
          </div>
          <div className="field">
            <span>
              Tries to re-plan
              <small>
                {settings?.controlMaxReplans === "unlimited"
                  ? "If a step fails, BYTE looks again and plans again as many times as it takes (until the run time limit or Stop)."
                  : "If a step doesn't work (a button moved, a window didn't open), BYTE looks at the screen again and plans again, up to this many times."}
              </small>
            </span>
            <select
              value={settings?.controlMaxReplans ?? "2"}
              aria-label="Tries to re-plan"
              onChange={(e) => void update({ controlMaxReplans: e.target.value as "2" | "5" | "10" | "unlimited" })}
            >
              <option value="2">2 times</option>
              <option value="5">5 times</option>
              <option value="10">10 times</option>
              <option value="unlimited">Unlimited</option>
            </select>
          </div>
          <label className="field">
            <span>
              Ask before risky steps
              <small>Shutting down or restarting, closing a window, deleting, and buttons like Send, Buy or Pay each show a card first. Turn off only if you trust every request.</small>
            </span>
            <input type="checkbox" checked={settings?.controlConfirmRisky ?? true} onChange={(e) => void update({ controlConfirmRisky: e.target.checked })} aria-label="Ask before risky steps" />
          </label>
          <label className="field">
            <span>
              Never ask, just do it
              <small>
                Turns off every approval card for PC control, including shutting down, closing windows and opening files and links. Choose this only when you trust the
                model that is doing the controlling. BYTE still refuses to run program files (.exe, scripts, installers) and a few dangerous key combinations, never
                runs in private chats, and the Stop button always works.
              </small>
            </span>
            <input
              type="checkbox"
              checked={settings?.controlNeverAsk ?? false}
              onChange={(e) => {
                if (e.target.checked && !window.confirm("Turn off every approval card for PC control? BYTE will shut down, close windows and open files and links without asking. Program files are still refused, and Stop always works.")) return;
                void update({ controlNeverAsk: e.target.checked });
              }}
              aria-label="Never ask, just do it"
            />
          </label>

          <div className="field" style={{ display: "block" }}>
            <span>
              Model that does the controlling
              <small>
                Now: {status?.modelLabel ?? "the chat model"}. A small, fast model that follows a strict format works best and can run beside your chat model. Leave it on the chat
                model if you only have one.
              </small>
            </span>
            <div className="column" style={{ gap: 6, marginTop: 8 }}>
              <label className="row" style={{ gap: 8 }}>
                <input type="radio" name="control-model" checked={!(settings?.controlModel ?? "")} onChange={() => void update({ controlModel: "" })} />
                <span>Use the chat model</span>
              </label>
              {(status?.models ?? []).map((m) => {
                const d = m.downloadKey ? downloads[m.downloadKey] : undefined;
                const busy = !!d && ["downloading", "resuming", "verifying"].includes(d.phase);
                return (
                  <div key={m.id} className="row" style={{ gap: 8, alignItems: "center" }}>
                    <input
                      type="radio"
                      name="control-model"
                      disabled={!m.installedKey}
                      checked={m.selected}
                      onChange={() => m.installedKey && void update({ controlModel: m.installedKey })}
                      aria-label={m.name}
                    />
                    <span className="grow">
                      {m.name}
                      <small>
                        {m.note} {m.installedKey ? "Downloaded." : `${sizeText(m.downloadBytes)} to download.`}
                      </small>
                    </span>
                    {!m.installedKey && m.downloadKey && (
                      <button className="btn sm" disabled={busy} onClick={() => void guard(() => controlApi.download(m.downloadKey!))}>
                        <Download size={14} /> {busy ? `${Math.min(99, Math.round(((d?.bytes ?? 0) / Math.max(1, m.downloadBytes)) * 100))}%` : "Download"}
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </div>

          <label className="field">
            <span>
              Keep the game fast
              <small>While a game is running, the control model works on the processor and system memory only, so the game keeps your graphics card. It uses more memory and answers a little slower then.</small>
            </span>
            <input type="checkbox" checked={settings?.controlGameRam ?? true} onChange={(e) => void update({ controlGameRam: e.target.checked })} aria-label="Keep the game fast" />
          </label>

          <div className="field" style={{ display: "block" }}>
            <span>
              Programs BYTE may use without asking
              <small>Empty means BYTE asks the first time it uses each program in a request. Names like “notepad” or “chrome”.</small>
            </span>
            <div className="row" style={{ gap: 6, flexWrap: "wrap", margin: "8px 0" }}>
              {allowed.map((a) => (
                <span key={a} className="chip">
                  {a}
                  <button className="icon-btn" aria-label={`Remove ${a}`} onClick={() => void update({ controlAllowedApps: allowed.filter((x) => x !== a) })}>
                    <X size={12} />
                  </button>
                </span>
              ))}
            </div>
            <form
              className="row"
              style={{ gap: 6 }}
              onSubmit={(e) => {
                e.preventDefault();
                const next = addApp(allowed, appDraft);
                if (next !== allowed) void update({ controlAllowedApps: next });
                setAppDraft("");
              }}
            >
              <input className="text-input" list="control-running" value={appDraft} placeholder="Program name" maxLength={60} onChange={(e) => setAppDraft(e.target.value)} onFocus={() => void controlApi.runningApps().then(setRunning, () => undefined)} />
              <datalist id="control-running">
                {running.slice(0, 80).map((r) => (
                  <option key={r} value={r} />
                ))}
              </datalist>
              <button className="btn sm" type="submit" disabled={!appDraft.trim()}>
                <Plus size={14} /> Add
              </button>
            </form>
          </div>
        </>
      )}

      <label className="field">
        <span>
          How's my PC?
          <small>Ask “how's my PC?” and BYTE reads out the graphics card's temperature and memory, the processor, free disk space, and what is using the most memory. Read from this PC, nothing sent anywhere.</small>
        </span>
        <input type="checkbox" checked={settings?.pcStatus ?? true} onChange={(e) => void update({ pcStatus: e.target.checked })} aria-label="How's my PC" />
      </label>

      <label className="field">
        <span>
          Guided help
          <small>
            Ask “how do I change my shipping address on this site?” and BYTE explains, then offers to ring each button in turn or to do it for you. To do that it reads the
            buttons and fields of the window in front (never password or card fields, never windows on your never-look list, never in private chats), on this PC only. Off until you turn it on.
          </small>
        </span>
        <input type="checkbox" checked={settings?.guidedHelp ?? false} onChange={(e) => void update({ guidedHelp: e.target.checked })} aria-label="Guided help" />
      </label>

      <label className="field">
        <span>
          Find files by voice
          <small>“Find the invoice from last week” looks only in the folders you choose below, on this PC, and shows the best match selected in File Explorer. Off until you turn it on.</small>
        </span>
        <input type="checkbox" checked={settings?.findFiles ?? false} onChange={(e) => void update({ findFiles: e.target.checked })} aria-label="Find files by voice" />
      </label>
      {(settings?.findFiles ?? false) && (
        <div className="field" style={{ display: "block" }}>
          <span>
            Folders BYTE may search
            <small>{folders.length === 0 ? "None yet: BYTE won't search anywhere until you add one." : "Only these, and what is inside them."}</small>
          </span>
          {folderError && <small className="error">{folderError}</small>}
          <ul className="plain-list" style={{ margin: "8px 0" }}>
            {folders.map((f) => (
              <li key={f} className="row" style={{ gap: 8 }}>
                <span className="grow">{f}</span>
                <button className="icon-btn" aria-label={`Remove ${f}`} onClick={() => void update({ findFolders: folders.filter((x) => x !== f) })}>
                  <Trash2 size={14} />
                </button>
              </li>
            ))}
          </ul>
          <button className="btn sm" onClick={() => void addFolder()}>
            <Plus size={14} /> Add a folder
          </button>
        </div>
      )}

      <RoutinesMenu settings={settings} />

      <div className="field" style={{ display: "block" }}>
        <span>
          What BYTE did
          <small>Every step BYTE took on this PC, kept only on this PC. What was typed is never written down, only how much.</small>
        </span>
        <div className="row" style={{ gap: 8, margin: "8px 0" }}>
          <button className="btn sm" onClick={refresh}>Refresh</button>
          <button className="btn sm ghost" disabled={log.length === 0} onClick={() => void controlApi.logClear().then(refresh)}>
            <Trash2 size={14} /> Clear
          </button>
        </div>
        {log.length === 0 ? (
          <small>Nothing yet.</small>
        ) : (
          <ul className="plain-list" aria-label="Activity log" style={{ maxHeight: 220, overflow: "auto" }}>
            {log.map((e, i) => (
              <li key={`${e.at}-${i}`}>
                <small>
                  {e.at} · {e.ok ? "✓" : "✗"} {e.what}
                  {e.detail ? ` — ${e.detail}` : ""}
                </small>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
