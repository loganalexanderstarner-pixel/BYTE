import { useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { bytes } from "../../lib/format";
import { foundApi, leafName, removeApi, type RemovePlan, type SafeStatus } from "../../lib/found";

/** Settings → Backup and help: safe mode. */
function SafeModeCard() {
  const [status, setStatus] = useState<SafeStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void foundApi.safeStatus().then(setStatus, () => undefined);
  }, []);
  const go = async (safe: boolean) => {
    if (safe && !window.confirm("Restart BYTE in safe mode? Anything running now stops. Your saved settings are not changed.")) return;
    try {
      await foundApi.safeRestart(safe);
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <div className="field" style={{ display: "block" }}>
      <span>
        Safe mode
        <small>
          Restarts BYTE with Hey BYTE, PC control, screen features, the browser and every other optional feature that listens, looks or controls switched off for that run. Your saved settings are not changed, so a normal start brings them back. You can also start BYTE with <code>--safe</code>. Use it if BYTE keeps closing or something behaves oddly.
        </small>
      </span>
      <button className="btn" style={{ marginTop: 6 }} onClick={() => void go(!status?.active)}>
        {status?.active ? "Restart normally" : "Restart in safe mode"}
      </button>
      {error && <small className="bad">{error}</small>}
    </div>
  );
}

/** Settings → Backup and help: remove BYTE's own data before uninstalling. */
function RemoveDataCard() {
  const [chats, setChats] = useState(false);
  const [models, setModels] = useState(false);
  const [plan, setPlan] = useState<RemovePlan | null>(null);
  const [word, setWord] = useState("");
  const [error, setError] = useState<string | null>(null);
  const look = async () => {
    setError(null);
    try {
      setPlan(await removeApi.plan(chats, models));
    } catch (e) {
      setError(errorText(e));
    }
  };
  const go = async () => {
    try {
      await removeApi.remove(chats, models, word);
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <div className="field" style={{ display: "block" }}>
      <span>
        Remove BYTE's data
        <small>
          Deletes what you choose, takes BYTE out of the startup list and closes BYTE so you can uninstall it. Your notes, saved backups and exported files are yours and are never touched. This cannot be undone: save a backup first if you might want anything back.
        </small>
      </span>
      <div role="group" aria-label="What to remove" style={{ display: "flex", gap: 16, flexWrap: "wrap", marginTop: 6 }}>
        <label>
          <input type="checkbox" aria-label="Remove chats, memories and settings" checked={chats} onChange={(e) => (setChats(e.target.checked), setPlan(null))} /> Chats, memories and settings
        </label>
        <label>
          <input type="checkbox" aria-label="Remove downloaded models" checked={models} onChange={(e) => (setModels(e.target.checked), setPlan(null))} /> Downloaded models
        </label>
      </div>
      {!plan && (
        <button className="btn" style={{ marginTop: 6 }} disabled={!chats && !models} onClick={() => void look()}>
          Show what would be removed
        </button>
      )}
      {plan && (
        <>
          <ul aria-label="Files and folders that will be removed">
            {plan.paths.map(([p, n]) => (
              <li key={p} title={p}>
                {leafName(p)} ({bytes(n)})
              </li>
            ))}
          </ul>
          <small>
            In total {bytes(plan.totalBytes)}. Type REMOVE to confirm.
          </small>
          <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
            <input className="text-input" value={word} onChange={(e) => setWord(e.target.value)} aria-label="Type REMOVE to confirm" placeholder="REMOVE" />
            <button className="btn danger" disabled={word.trim() !== "REMOVE"} onClick={() => void go()}>
              Remove and close BYTE
            </button>
            <button className="btn" onClick={() => setPlan(null)}>
              Cancel
            </button>
          </div>
        </>
      )}
      {error && <small className="bad">{error}</small>}
    </div>
  );
}

/** The foundations track's cards for Settings → Backup and help. */
export function FoundBackupCards() {
  if (!inTauri) return null;
  return (
    <>
      <SafeModeCard />
      <RemoveDataCard />
    </>
  );
}
