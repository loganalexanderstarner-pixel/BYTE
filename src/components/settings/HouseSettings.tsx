import { useEffect, useState } from "react";
import { Trash2 } from "lucide-react";

import { api, errorText, inTauri, type DiskItems } from "../../lib/api";
import { bytes } from "../../lib/format";
import { useStore } from "../../state/store";

/** The small on/off settings of the house track (disk hints, game and meeting awareness, spoken notices, recap). */
export function HouseToggles({ group }: { group?: "models" | "voice" | "memory" }) {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const all: { key: "diskHints" | "gameAware" | "meetingQuiet" | "spokenNotices" | "recap"; fallback: boolean; title: string; text: string }[] = [
    { key: "diskHints", fallback: true, title: "Disk hints", text: "Before a download and when the disk is low, say in plain words how much is needed, how much is free and what BYTE's own files could be cleared. Nothing is ever deleted by itself." },
    { key: "gameAware", fallback: true, title: "Game-aware", text: "While a game is running, BYTE answers spoken questions from BYTE Cloud (if you are connected), plays no chime after Hey BYTE and holds back spoken notices (timers still ring)." },
    { key: "meetingQuiet", fallback: true, title: "Quiet during calls", text: "Spoken notices wait while another program is using the microphone for a call (only who holds the microphone is checked; nothing is recorded). “Hey BYTE” itself is also switched off during a call, so BYTE does not answer the room (the listening pill says so); it comes back a few seconds after the call ends." },
    { key: "spokenNotices", fallback: false, title: "Spoken notices", text: "Say “your model finished downloading” and long jobs finishing out loud. Only when speech is on, and never during a game or a call." },
    { key: "recap", fallback: true, title: "Where did I leave off", text: "Ask “where did I leave off?” for a recap of your latest chats and notes, and say “note: …” to save a quick note. Built from your own chats and notes, nothing is invented." },
  ];
  const [time, setTime] = useState(settings?.scheduledBriefingTime ?? "07:30");
  const sync = (enabled: boolean, t: string) => void api.houseBriefingSync(enabled, t).catch(() => undefined);
  const where: Record<string, string> = { diskHints: "models", gameAware: "models", meetingQuiet: "voice", spokenNotices: "voice", recap: "memory" };
  const rows = all.filter((r) => !group || where[r.key] === group);
  return (
    <>
      {rows.map((r) => (
        <label className="field" key={r.key}>
          <span>
            {r.title}
            <small>{r.text}</small>
          </span>
          <input type="checkbox" checked={settings?.[r.key] ?? r.fallback} onChange={(e) => void update({ [r.key]: e.target.checked })} />
        </label>
      ))}
      {(!group || group === "voice") && (
        <label className="field">
          <span>
            Quiet hours
            <small>
              Between these times “Hey BYTE” is switched off (the listening pill says so) and BYTE says nothing on its own: no spoken notices or briefing. Typing to BYTE and the talk key still work. Off until you turn it on.
            </small>
          </span>
          <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input type="time" value={settings?.quietFrom ?? "22:00"} aria-label="Quiet hours start" onChange={(e) => void update({ quietFrom: e.target.value || "22:00" })} />
            <span aria-hidden="true">to</span>
            <input type="time" value={settings?.quietTo ?? "07:00"} aria-label="Quiet hours end" onChange={(e) => void update({ quietTo: e.target.value || "07:00" })} />
            <input type="checkbox" checked={settings?.quietHours ?? false} aria-label="Quiet hours" onChange={(e) => void update({ quietHours: e.target.checked })} />
          </span>
        </label>
      )}
      {(!group || group === "voice") && (
        <label className="field">
          <span>
            Spoken morning briefing
            <small>Every day at the time you choose, BYTE puts your briefing together, saves it as a chat and says the summary out loud (never during a game or a call).</small>
          </span>
          <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input
              type="time"
              value={time}
              aria-label="Briefing time"
              onChange={(e) => setTime(e.target.value)}
              onBlur={() => {
                void update({ scheduledBriefingTime: time || "07:30" });
                if (settings?.scheduledBriefing) sync(true, time || "07:30");
              }}
            />
            <input
              type="checkbox"
              checked={settings?.scheduledBriefing ?? false}
              onChange={(e) => {
                void update({ scheduledBriefing: e.target.checked, scheduledBriefingTime: time || "07:30" });
                sync(e.target.checked, time || "07:30");
              }}
            />
          </span>
        </label>
      )}
    </>
  );
}

/** Settings card: BYTE's own big files with a delete button per item that asks first. */
export function FreeUpSpace() {
  const settings = useStore((s) => s.settings);
  const [d, setD] = useState<DiskItems | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = () => void (inTauri ? api.diskItems() : Promise.resolve(null)).then(setD, (e) => setError(errorText(e)));
  useEffect(refresh, []);
  if (settings?.diskHints === false) return null;
  const remove = async (id: string, label: string) => {
    const { ask } = await import("@tauri-apps/plugin-dialog");
    if (!(await ask(`Delete “${label}”? This cannot be undone.`, { title: "Free up space", kind: "warning" }))) return;
    await api.diskDeleteItem(id).then(refresh, (e) => setError(errorText(e)));
  };
  return (
    <div className="field" style={{ display: "block" }}>
      <span>
        Free up space
        <small>{d ? `${d.freeWords} free on the disk BYTE uses. ` : ""}{d?.low ?? "These are BYTE's own big files. Nothing is deleted unless you press a button and confirm."}</small>
      </span>
      {error && <small className="bad">{error}</small>}
      {d && d.items.length === 0 && <small>Nothing big to clear.</small>}
      {d?.items.slice(0, 12).map((i) => (
        <div key={i.id} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8, padding: "4px 0" }}>
          <span>
            {i.label} <small>{bytes(i.bytes)}</small>
          </span>
          <button className="icon-btn sm" title={`Delete ${i.label}`} aria-label={`Delete ${i.label}`} onClick={() => void remove(i.id, i.label)}>
            <Trash2 size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}

/** Settings card: save settings and chats to a file you pick (passphrase-protected), and restore from one. */
export function BackupRestoreCard() {
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  // After the restart: what the restore did (once).
  useEffect(() => {
    if (!inTauri) return;
    void api.houseRestoreReport().then((r) => {
      if (!r) return;
      setMsg(r.ok ? { ok: true, text: `Restored ${r.chats} chat${r.chats === 1 ? "" : "s"}${r.skipped ? ` (${r.skipped} could not be read and were skipped)` : ""} and your settings. A copy of what was replaced is in ${r.keptIn ?? "BYTE's data folder"}.` } : { ok: false, text: `The restore did not finish: ${r.error ?? "unknown problem"}. Nothing was changed.` });
    }, () => undefined);
  }, []);
  if (!inTauri) return null;
  const tooShort = pass.length < 8;
  const save = async () => {
    setBusy(true);
    setMsg(null);
    try {
      const { save: pickSave } = await import("@tauri-apps/plugin-dialog");
      const path = await pickSave({ title: "Save a copy of your settings and chats", defaultPath: "BYTE settings and chats.bytecopy", filters: [{ name: "BYTE copy", extensions: ["bytecopy"] }] });
      if (!path) return;
      const n = await api.houseBackupSave(path, pass);
      setMsg({ ok: true, text: `Saved ${n} chat${n === 1 ? "" : "s"} and your settings. Keep the passphrase: without it the file cannot be opened. Your BYTE Cloud key and account details are not in the file.` });
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  };
  const restore = async () => {
    setBusy(true);
    setMsg(null);
    try {
      const { open: pickOpen, ask } = await import("@tauri-apps/plugin-dialog");
      const path = await pickOpen({ title: "Choose a BYTE settings and chats file", multiple: false, filters: [{ name: "BYTE copy", extensions: ["bytecopy"] }] });
      if (typeof path !== "string") return;
      if (!(await ask("Restoring puts the saved chats back and applies the saved settings, then BYTE restarts. Chats with the same name are replaced by the saved version. Continue?", { title: "Restore", kind: "warning" }))) return;
      await api.houseBackupRestore(path, pass);
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="field" style={{ display: "block" }}>
      <span>
        Save and restore settings and chats
        <small>Save a copy to a file you choose (a USB drive, a cloud folder), sealed with a passphrase. Restore it on this or another PC; BYTE restarts to finish.</small>
      </span>
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginTop: 6 }}>
        <input type="password" value={pass} onChange={(e) => setPass(e.target.value)} placeholder="Passphrase (8 or more characters)" aria-label="Passphrase" autoComplete="off" />
        <button className="btn" disabled={busy || tooShort} onClick={() => void save()}>
          Save a copy…
        </button>
        <button className="btn" disabled={busy || tooShort} onClick={() => void restore()}>
          Restore from a file…
        </button>
      </div>
      {msg && <small className={msg.ok ? undefined : "bad"}>{msg.text}</small>}
    </div>
  );
}

/** Settings → Backup and help: build a problem report, show exactly what is in it, then save it to a file. */
export function ProblemReportCard() {
  const [report, setReport] = useState<{ text: string; includes: string[] } | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  if (!inTauri) return null;
  const build = async () => {
    setBusy(true);
    setMsg(null);
    try {
      const r = await api.problemReportBuild();
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
      await api.problemReportSave(path, text);
      setMsg({ ok: true, text: "Saved. Attach the file when you tell us about the problem. Nothing was sent anywhere." });
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    }
  };
  return (
    <div className="field" style={{ display: "block" }}>
      <span>
        Problem report
        <small>Collects BYTE's version, your system and hardware names and the end of BYTE's log, with your name, your user folder and anything that looks like a key taken out. You see exactly what is in it before you save it. Nothing is sent anywhere.</small>
      </span>
      {!report && (
        <button className="btn" disabled={busy} onClick={() => void build()} style={{ marginTop: 6 }}>
          {busy ? "Collecting…" : "Prepare a report"}
        </button>
      )}
      {report && (
        <>
          <small>Includes: {report.includes.join("; ")}. You can edit or delete lines below before saving.</small>
          <textarea value={text} onChange={(e) => setText(e.target.value)} rows={12} style={{ width: "100%", fontFamily: "monospace", fontSize: 12, marginTop: 6 }} aria-label="Problem report preview" />
          <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
            <button className="btn" onClick={() => void save()}>
              Save to a file…
            </button>
            <button className="btn" onClick={() => setReport(null)}>
              Cancel
            </button>
          </div>
        </>
      )}
      {msg && <small className={msg.ok ? undefined : "bad"}>{msg.text}</small>}
    </div>
  );
}
