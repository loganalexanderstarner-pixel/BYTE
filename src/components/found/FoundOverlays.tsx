import { useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { listen } from "@tauri-apps/api/event";
import { useStore } from "../../state/store";
import { renderMarkdown } from "../../lib/markdown";
import { batteryBadgeText, crashApi, crashCardText, foundApi, powerApi, smallerModel, type PowerStatus, safeBannerText, whatsNewApi, whatsNewParts, type CrashFound, type SafeStatus, type WhatsNew } from "../../lib/found";

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
    <div className="banner crash-card" role="alertdialog" aria-labelledby="crash-title" style={{ flexWrap: "wrap" }} onKeyDown={(e) => e.key === "Escape" && close()}>
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

/** After an update: this version's notes, once. */
function WhatsNewCard() {
  const [info, setInfo] = useState<WhatsNew | null>(null);
  useEffect(() => {
    if (inTauri) void whatsNewApi.status().then((w) => w.show && setInfo(w), () => undefined);
  }, []);
  if (!info) return null;
  const { title, body } = whatsNewParts(info.notes);
  const close = () => {
    void whatsNewApi.seen();
    setInfo(null);
  };
  return (
    <div className="banner whatsnew-card" role="dialog" aria-labelledby="whatsnew-title" style={{ display: "block", maxHeight: "60vh", overflow: "auto" }} onKeyDown={(e) => e.key === "Escape" && close()}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <b className="grow" id="whatsnew-title">
          What's new in BYTE {info.version}: {title}
        </b>
        <button className="btn sm primary" onClick={close}>
          Got it
        </button>
      </div>
      {/* The notes are bundled with the app and go through the same sanitising as every other text. */}
      <div
        className="md"
        onClick={(e) => {
          const a = (e.target as HTMLElement).closest("a");
          if (!a) return;
          e.preventDefault();
          const href = a.getAttribute("href") ?? "";
          if (/^https?:/i.test(href)) void import("@tauri-apps/plugin-opener").then((m) => m.openUrl(href));
        }}
        dangerouslySetInnerHTML={{ __html: renderMarkdown(body) }}
      />
    </div>
  );
}

/** Laptop mode: a small badge while on battery, with a one-click smaller model when one is downloaded. */
function BatteryBadge() {
  const [p, setP] = useState<PowerStatus | null>(null);
  const models = useStore((s) => s.models);
  const active = useStore((s) => s.settings?.activeModel);
  const update = useStore((s) => s.updateSettings);
  const laptop = useStore((s) => s.settings?.laptopMode);
  useEffect(() => {
    if (!inTauri) return;
    void powerApi.status().then(setP, () => undefined);
    const off = listen<PowerStatus>("power://state", (e) => setP(e.payload));
    return () => void off.then((f) => f());
  }, [laptop]);
  if (!p || !laptop || !p.onBattery) return null;
  const smaller = smallerModel(models, active);
  return (
    <div className="banner battery-badge" role="status" aria-live="polite">
      <span className="grow">{batteryBadgeText({ ...p, laptopMode: true })}</span>
      {smaller && (
        <button
          className="btn sm"
          onClick={() => void update({ activeModel: smaller.key }).then(() => import("../../lib/api").then((m) => m.api.modelActivate(smaller.key)))}
          title="Uses less power than the model in use"
        >
          Use {smaller.name}
        </button>
      )}
    </div>
  );
}

/** Everything the foundations track shows over the app: one mount point in the shell. */
export function FoundOverlays() {
  return (
    <>
      <SafeModeBanner />
      <CrashRecoveryCard />
      <WhatsNewCard />
      <BatteryBadge />
    </>
  );
}
