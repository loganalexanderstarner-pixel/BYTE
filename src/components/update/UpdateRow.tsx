import { Download, Loader2, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import type { UpdateInfo } from "../../lib/types";
import { useStore } from "../../state/store";

/** Settings → About: check for a newer BYTE and install it in one click (updater.rs). */
export function UpdateRow() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [found, setFound] = useState<UpdateInfo | null | undefined>(undefined);
  const [busy, setBusy] = useState<"check" | "install" | null>(null);
  const [progress, setProgress] = useState<{ got: number; total: number | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [configured, setConfigured] = useState(false);

  useEffect(() => {
    if (!inTauri) return;
    void api.updateConfigured().then(setConfigured, () => undefined);
    const off = api.onUpdateProgress(setProgress);
    return () => void off.then((f) => f());
  }, []);

  const check = async () => {
    setBusy("check");
    setError(null);
    try {
      setFound(await api.updateCheck());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };
  const install = async () => {
    setBusy("install");
    setError(null);
    try {
      await api.updateInstall();
    } catch (e) {
      setError(errorText(e));
      setBusy(null);
    }
  };

  // Builds made before the signing key existed can't update themselves.
  if (!configured) return null;
  const pct = progress?.total ? Math.round((progress.got / progress.total) * 100) : null;
  return (
    <div className="field" style={{ display: "block" }}>
      <div className="row" style={{ gap: 8, alignItems: "center", flexWrap: "wrap" }}>
        <span className="grow">
          Updates
          <small>
            {found === undefined
              ? "BYTE can download and install new versions itself; each one is checked against BYTE's signature first."
              : found
                ? `BYTE ${found.version} is out (you have ${found.current}).`
                : "You have the latest version."}
          </small>
        </span>
        {found ? (
          <button className="btn sm primary" onClick={() => void install()} disabled={!!busy}>
            {busy === "install" ? <Loader2 size={13} className="spin" /> : <Download size={13} />}
            {busy === "install" ? (pct != null ? `Installing… ${pct}%` : "Installing…") : `Install ${found.version} and restart`}
          </button>
        ) : (
          <button className="btn sm ghost" onClick={() => void check()} disabled={!!busy}>
            {busy === "check" ? <Loader2 size={13} className="spin" /> : <RefreshCw size={13} />} Check for updates
          </button>
        )}
      </div>
      <label className="row small" style={{ gap: 6, marginTop: 6 }}>
        <input type="checkbox" checked={settings?.updateCheck !== false} onChange={(e) => void update({ updateCheck: e.target.checked })} /> Check once a day and tell me
      </label>
      {found?.notes && <p className="faint small update-notes">{found.notes.split("\n").find((l) => l.trim() && !l.startsWith("#"))}</p>}
      {error && <div className="banner danger">{error}</div>}
    </div>
  );
}
