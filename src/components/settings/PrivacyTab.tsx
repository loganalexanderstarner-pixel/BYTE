import { Check, Copy, ExternalLink, Fingerprint, Lock, RefreshCw, Search, Trash2, WifiOff, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { argsLine, byDay, KINDS, kindLabel, lockAfterLabel, toolLabel } from "../../lib/privacy";
import type { Activity, ActivityKind, LockStatus, Permission } from "../../lib/types";
import { useStore, type SettingsTab } from "../../state/store";

/** Settings → Privacy: the offline switch, the lock, Mac permissions and everything BYTE did. */
export function PrivacyTab() {
  return (
    <div className="privacy-tab">
      <OfflineSection />
      <LockSection />
      <PermissionsSection />
      <ActivitySection />
    </div>
  );
}

function OfflineSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const openSettings = useStore((s) => s.openSettings);
  const offline = !!settings?.offline;
  const go = (tab: SettingsTab) => openSettings(tab);
  const rows: { name: string; state: string; on: boolean; tab?: SettingsTab }[] = [
    { name: "Web search and reading pages", state: settings?.webSearch === false ? "Off" : settings?.webMode === "always" ? "Always" : "When a question needs it", on: settings?.webSearch !== false, tab: "engine" },
    { name: "BYTE Cloud", state: settings?.cloudConnected ? "Connected" : "Not connected", on: !!settings?.cloudConnected, tab: "cloud" },
    { name: "Connectors (Notion, calendar links)", state: settings?.connectorsEnabled === false ? "Off" : "Only the ones you set up", on: settings?.connectorsEnabled !== false, tab: "connectors" },
    { name: "News feeds and watched pages", state: settings?.watchEnabled === false ? "Off" : "Checked in the background", on: settings?.watchEnabled !== false },
    { name: "Downloads (models, voices, the video helper)", state: "Only when you press Download", on: true, tab: "models" },
  ];
  return (
    <section className="settings-section">
      <h3>
        <WifiOff size={16} /> Offline switch
      </h3>
      <div className="field" style={{ display: "block" }}>
        <label className="row" style={{ alignItems: "flex-start", gap: 12 }}>
          <span className="grow">
            Work offline
            <small>
              Nothing leaves this Mac while this is on: no web search, no cloud, no downloads, no background checks. Chats, voices, your files and notes keep working with the model on
              this Mac. Also in the menu-bar icon's menu and ⌘K.
            </small>
          </span>
          <input type="checkbox" checked={offline} onChange={(e) => void update({ offline: e.target.checked })} aria-label="Work offline" />
        </label>
      </div>
      <p className="faint small">What can reach the internet{offline ? " (all blocked while offline)" : ""}:</p>
      <ul className="privacy-list">
        {rows.map((r) => (
          <li key={r.name} className={offline ? "blocked" : r.on ? "" : "off"}>
            <span className="grow">{r.name}</span>
            <span className="faint small">{offline ? "Blocked" : r.state}</span>
            {r.tab && (
              <button className="linklike small" onClick={() => go(r.tab!)}>
                Settings
              </button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

const IDLE_CHOICES = [0, 5, 15, 60];

function LockSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [status, setStatus] = useState<LockStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (inTauri) void api.lockStatus().then(setStatus, () => undefined);
  }, [settings?.lockEnabled]);
  const on = !!settings?.lockEnabled;
  const after = settings?.lockAfterMinutes ?? 15;

  const toggle = async (want: boolean) => {
    setError(null);
    setBusy(true);
    try {
      // Turning it on: first make sure this Mac can unlock it again.
      if (want) await api.lockVerify();
      await update({ lockEnabled: want });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-section">
      <h3>
        <Fingerprint size={16} /> Lock
      </h3>
      {status && !status.available ? (
        <p className="faint small">This Mac can't confirm it's you (no Touch ID and no password set up), so BYTE can't be locked here.</p>
      ) : (
        <>
          <div className="field" style={{ display: "block" }}>
            <label className="row" style={{ alignItems: "flex-start", gap: 12 }}>
              <span className="grow">
                Lock BYTE with Touch ID
                <small>
                  BYTE asks for Touch ID (or your Mac's password) when it opens, and after it sits unused. While locked, chats, notes, memories and the activity log stay hidden, and
                  Quick Ask asks too.
                </small>
              </span>
              <input type="checkbox" checked={on} disabled={busy} onChange={(e) => void toggle(e.target.checked)} aria-label="Lock BYTE with Touch ID" />
            </label>
          </div>
          {on && (
            <div className="row" style={{ gap: 8, flexWrap: "wrap", marginBottom: 8 }}>
              <select value={after} onChange={(e) => void update({ lockAfterMinutes: Number(e.target.value) })} aria-label="When to lock">
                {IDLE_CHOICES.map((m) => (
                  <option key={m} value={m}>
                    {lockAfterLabel(m)}
                  </option>
                ))}
              </select>
              <button className="btn sm ghost" onClick={() => void api.lockNow().catch((e) => setError(errorText(e)))}>
                <Lock size={13} /> Lock now
              </button>
            </div>
          )}
          {error && <div className="banner danger">{error}</div>}
          <p className="faint small">
            Your chats are already encrypted on this Mac. The lock keeps someone using your unlocked Mac out of BYTE. (Tying the encryption key itself to Touch ID needs a paid Apple
            developer signature, which BYTE doesn't use.)
          </p>
        </>
      )}
    </section>
  );
}

const STATUS_LABEL: Record<Permission["status"], string> = { allowed: "Allowed", denied: "Not allowed", "not asked": "Not yet", unknown: "Asked when first used" };

function PermissionsSection() {
  const [perms, setPerms] = useState<Permission[]>([]);
  const load = useCallback(() => {
    if (inTauri) void api.privacyPermissions().then(setPerms, () => undefined);
  }, []);
  useEffect(() => {
    load();
    // Coming back from System Settings: show the new state.
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);
  return (
    <section className="settings-section">
      <h3>Mac permissions</h3>
      <p className="faint small">macOS asks the first time BYTE needs each one. Change them any time in System Settings → Privacy &amp; Security.</p>
      <ul className="privacy-list perms">
        {perms.map((p) => (
          <li key={p.id}>
            <span className="grow">
              <b>{p.name}</b>
              <small className="faint">{p.why}</small>
            </span>
            <span className={`perm-status ${p.status.replace(" ", "-")}`}>
              {p.status === "allowed" && <Check size={12} />}
              {p.status === "denied" && <X size={12} />}
              {STATUS_LABEL[p.status]}
            </span>
            <button className="btn sm ghost" onClick={() => void api.upkeepOpenSettings(p.url)} title="Open in System Settings">
              <ExternalLink size={13} /> Open
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function ActivitySection() {
  const [list, setList] = useState<Activity[]>([]);
  const [kind, setKind] = useState<ActivityKind | null>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<number | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    if (!inTauri) return;
    void api.actionsList(query || undefined, kind ?? undefined, 300).then(setList, (e) => setError(errorText(e)));
  }, [query, kind]);
  useEffect(() => {
    const t = setTimeout(load, 200);
    return () => clearTimeout(t);
  }, [load]);
  const groups = useMemo(() => byDay(list), [list]);

  const clear = async () => {
    if (!window.confirm("Clear BYTE's whole activity log? This can't be undone.")) return;
    await api.actionsClear().catch((e) => setError(errorText(e)));
    load();
  };
  const copy = async () => {
    await navigator.clipboard.writeText(JSON.stringify(list, null, 2));
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  let n = 0;
  return (
    <section className="settings-section">
      <h3>Activity</h3>
      <p className="faint small">Everything BYTE did for you: searches, pages read, Mac actions, commands, files, connectors and automations. Kept on this Mac only.</p>
      <div className="row" style={{ gap: 6, flexWrap: "wrap", marginBottom: 8 }}>
        <label className="search grow">
          <Search size={14} className="faint" />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search the activity" aria-label="Search the activity" />
        </label>
        <button className="icon-btn" onClick={load} title="Refresh">
          <RefreshCw size={14} />
        </button>
        <button className="btn sm ghost" onClick={() => void copy()} disabled={!list.length}>
          {copied ? <Check size={13} /> : <Copy size={13} />} Copy as JSON
        </button>
        <button className="btn sm ghost" onClick={() => void clear()} disabled={!list.length}>
          <Trash2 size={13} /> Clear
        </button>
      </div>
      <div className="chips" style={{ marginBottom: 8 }}>
        <button className="chip" aria-pressed={kind === null} onClick={() => setKind(null)}>
          All
        </button>
        {KINDS.map((k) => (
          <button key={k.id} className="chip" aria-pressed={kind === k.id} onClick={() => setKind(k.id)}>
            {k.label}
          </button>
        ))}
      </div>
      {error && <div className="banner danger">{error}</div>}
      {!list.length && <p className="faint small">Nothing here{query || kind ? " that matches" : " yet"}.</p>}
      <div className="activity-log">
        {groups.map((g) => (
          <div key={g.day}>
            <h4>{g.day}</h4>
            {g.items.map((a) => {
              const i = n++;
              const detail = argsLine(a.args);
              return (
                <div key={i} className={`activity-row ${a.ok ? "" : "failed"}`}>
                  <button className="activity-main" onClick={() => setOpen(open === i ? null : i)} aria-expanded={open === i}>
                    <span className="activity-time faint small">{new Date(a.ts).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}</span>
                    <span className={`activity-kind kind-${a.kind}`}>{kindLabel(a.kind)}</span>
                    <span className="grow">
                      {toolLabel(a.tool)}
                      {detail && <span className="faint"> · {detail}</span>}
                    </span>
                    {!a.ok && <span className="perm-status denied">Didn't work</span>}
                  </button>
                  {open === i && (
                    <div className="activity-detail small">
                      {a.summary && <p>{a.summary}</p>}
                      <pre>{JSON.stringify(a.args, null, 2)}</pre>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </section>
  );
}
