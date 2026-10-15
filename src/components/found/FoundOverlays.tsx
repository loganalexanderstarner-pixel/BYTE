import { useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { crashApi, crashCardText, foundApi, safeBannerText, type CrashFound, type SafeStatus } from "../../lib/found";

/** Safe mode banner: shown at the top for the whole run, with the way back to normal. */
function SafeModeBanner() {
  const [status, setStatus] = useState<SafeStatus | null>(null);
  useEffect(() => {
    if (inTauri) void foundApi.safeStatus().then(setStatus, () => undefined);
  }, []);
  if (!status?.active) return null;
  return (
    <div className="banner safe-banner" role="status">
      <span className="grow">{safeBannerText(status)}</span>
      <button className="btn sm" onClick={() => void foundApi.safeRestart(false)}>
        Restart normally
      </button>
    </div>
  );
}

/** After an unclean exit: offers a problem report, which the person reads and saves; nothing is sent anywhere. */
function CrashRecoveryCard() {
  const [found, setFound] = useState<CrashFound | null>(null);
  const [report, setReport] = useState<{ text: string; includes: string[] } | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  useEffect(() => {
    if (inTauri) void crashApi.status().then((r) => setFound(r.found), () => undefined);
  }, []);
  if (!found) return null;
  const close = () => {
    void crashApi.dismiss();
    setFound(null);
  };
  const prepare = async () => {
    setBusy(true);
    setMsg(null);
    try {
      const r = await crashApi.build();
      setReport(r);
      setText(r.text);
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  };
  const save = async () => {
    try {
      const { save: pickSave } = await import("@tauri-apps/plugin-dialog");
      const path = await pickSave({ title: "Save the problem report", defaultPath: "BYTE problem report.txt", filters: [{ name: "Text", extensions: ["txt"] }] });
      if (!path) return;
      await crashApi.save(path, text);
      setMsg({ ok: true, text: "Saved. Attach the file when you tell us about the problem. Nothing was sent anywhere." });
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    }
  };
  return (
    <div className="banner crash-card" role="alertdialog" aria-labelledby="crash-title" style={{ flexWrap: "wrap" }}>
      <span className="grow" id="crash-title">
        {crashCardText(found)}
      </span>
      {!report && (
        <>
          <button className="btn sm primary" disabled={busy} onClick={() => void prepare()}>
            {busy ? "Collecting…" : "Prepare a problem report"}
          </button>
          <button className="btn sm" onClick={() => void foundApi.safeRestart(true)} title="Restart with every optional feature off, without changing your settings">
            Restart in safe mode
          </button>
          <button className="btn sm ghost" onClick={close}>
            Not now
          </button>
        </>
      )}
      {report && (
        <div style={{ width: "100%" }}>
          <small>Includes: {report.includes.join("; ")}. You can edit or delete lines before saving. Nothing is sent anywhere.</small>
          <textarea value={text} onChange={(e) => setText(e.target.value)} rows={10} style={{ width: "100%", fontFamily: "monospace", fontSize: 12, marginTop: 6 }} aria-label="Problem report preview" />
          <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
            <button className="btn sm primary" onClick={() => void save()}>
              Save to a file…
            </button>
            <button className="btn sm" onClick={close}>
              Close
            </button>
          </div>
        </div>
      )}
      {msg && <small className={msg.ok ? undefined : "bad"}>{msg.text}</small>}
    </div>
  );
}

/** Everything the foundations track shows over the app: one mount point in the shell. */
export function FoundOverlays() {
  return (
    <>
      <SafeModeBanner />
      <CrashRecoveryCard />
    </>
  );
}
