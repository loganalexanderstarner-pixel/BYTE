import { useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { foundApi, type SafeStatus } from "../../lib/found";

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

/** The foundations track's cards for Settings → Backup and help. */
export function FoundBackupCards() {
  if (!inTauri) return null;
  return (
    <>
      <SafeModeCard />
    </>
  );
}
