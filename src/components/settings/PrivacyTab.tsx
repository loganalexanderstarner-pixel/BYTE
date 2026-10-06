import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  Archive,
  Baby,
  Check,
  MessageCircle,
  Cloud,
  Copy,
  ExternalLink,
  Fingerprint,
  Lock,
  RefreshCw,
  Search,
  Trash2,
  WifiOff,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import {
  argsLine,
  backupAge,
  byDay,
  formatBytes,
  KINDS,
  kindLabel,
  lockAfterLabel,
  toolLabel,
} from "../../lib/privacy";
import type {
  Activity,
  ActivityKind,
  BackupInfo,
  LockStatus,
  MessagesStatus,
  Permission,
} from "../../lib/types";
import { currentDevice } from "../../lib/device";
import { useStore, type SettingsTab } from "../../state/store";

/** Settings → Privacy: the offline switch, the lock, Mac permissions and everything BYTE did. */
export function PrivacyTab() {
  const phone = currentDevice() === "phone";
  return (
    <div className="privacy-tab">
      <OfflineSection />
      {/* Touch ID, iMessage, iCloud backups and macOS permissions are Mac-only; the phone gets its own later (A3). */}
      {!phone && <LockSection />}
      <KidsSection />
      {!phone && <MessagesSection />}
      {!phone && <BackupSection />}
      <OldChatsSection />
      {!phone && <PermissionsSection />}
      <ActivitySection />
      <EraseSection />
    </div>
  );
}

function OfflineSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const openSettings = useStore((s) => s.openSettings);
  const offline = !!settings?.offline;
  const go = (tab: SettingsTab) => openSettings(tab);
  const rows: {
    name: string;
    state: string;
    on: boolean;
    tab?: SettingsTab;
  }[] = [
    {
      name: "Web search and reading pages",
      state:
        settings?.webSearch === false
          ? "Off"
          : settings?.webMode === "always"
            ? "Always"
            : "When a question needs it",
      on: settings?.webSearch !== false,
      tab: "engine",
    },
    {
      name: "BYTE Cloud",
      state: settings?.cloudConnected ? "Connected" : "Not connected",
      on: !!settings?.cloudConnected,
      tab: "cloud",
    },
    {
      name: "Connectors (Notion, calendar links)",
      state:
        settings?.connectorsEnabled === false
          ? "Off"
          : "Only the ones you set up",
      on: settings?.connectorsEnabled !== false,
      tab: "connectors",
    },
    {
      name: "News feeds and watched pages",
      state:
        settings?.watchEnabled === false ? "Off" : "Checked in the background",
      on: settings?.watchEnabled !== false,
    },
    {
      name: "Downloads (models, voices, the video helper)",
      state: "Only when you press Download",
      on: true,
      tab: "models",
    },
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
              Nothing leaves this Mac while this is on: no web search, no cloud,
              no downloads, no background checks. Chats, voices, your files and
              notes keep working with the model on this Mac. Also in the
              menu-bar icon's menu and ⌘K.
            </small>
          </span>
          <input
            type="checkbox"
            checked={offline}
            onChange={(e) => void update({ offline: e.target.checked })}
            aria-label="Work offline"
          />
        </label>
      </div>
      <p className="faint small">
        What can reach the internet
        {offline ? " (all blocked while offline)" : ""}:
      </p>
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
        <p className="faint small">
          This Mac can't confirm it's you (no Touch ID and no password set up),
          so BYTE can't be locked here.
        </p>
      ) : (
        <>
          <div className="field" style={{ display: "block" }}>
            <label
              className="row"
              style={{ alignItems: "flex-start", gap: 12 }}
            >
              <span className="grow">
                Lock BYTE with Touch ID
                <small>
                  BYTE asks for Touch ID (or your Mac's password) when it opens,
                  and after it sits unused. While locked, chats, notes, memories
                  and the activity log stay hidden, and Quick Ask asks too.
                </small>
              </span>
              <input
                type="checkbox"
                checked={on}
                disabled={busy}
                onChange={(e) => void toggle(e.target.checked)}
                aria-label="Lock BYTE with Touch ID"
              />
            </label>
          </div>
          {on && (
            <div
              className="row"
              style={{ gap: 8, flexWrap: "wrap", marginBottom: 8 }}
            >
              <select
                value={after}
                onChange={(e) =>
                  void update({ lockAfterMinutes: Number(e.target.value) })
                }
                aria-label="When to lock"
              >
                {IDLE_CHOICES.map((m) => (
                  <option key={m} value={m}>
                    {lockAfterLabel(m)}
                  </option>
                ))}
              </select>
              <button
                className="btn sm ghost"
                onClick={() =>
                  void api.lockNow().catch((e) => setError(errorText(e)))
                }
              >
                <Lock size={13} /> Lock now
              </button>
            </div>
          )}
          {error && <div className="banner danger">{error}</div>}
          <p className="faint small">
            Your chats are already encrypted on this Mac. The lock keeps someone
            using your unlocked Mac out of BYTE. (Tying the encryption key
            itself to Touch ID needs a paid Apple developer signature, which
            BYTE doesn't use.)
          </p>
        </>
      )}
    </section>
  );
}

const STATUS_LABEL: Record<Permission["status"], string> = {
  allowed: "Allowed",
  denied: "Not allowed",
  "not asked": "Not yet",
  unknown: "Asked when first used",
};

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
      <p className="faint small">
        macOS asks the first time BYTE needs each one. Change them any time in
        System Settings → Privacy &amp; Security.
      </p>
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
            <button
              className="btn sm ghost"
              onClick={() => void api.upkeepOpenSettings(p.url)}
              title="Open in System Settings"
            >
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
    void api
      .actionsList(query || undefined, kind ?? undefined, 300)
      .then(setList, (e) => setError(errorText(e)));
  }, [query, kind]);
  useEffect(() => {
    const t = setTimeout(load, 200);
    return () => clearTimeout(t);
  }, [load]);
  const groups = useMemo(() => byDay(list), [list]);

  const clear = async () => {
    if (
      !window.confirm("Clear BYTE's whole activity log? This can't be undone.")
    )
      return;
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
      <p className="faint small">
        Everything BYTE did for you: searches, pages read, Mac actions,
        commands, files, connectors and automations. Kept on this Mac only.
      </p>
      <div
        className="row"
        style={{ gap: 6, flexWrap: "wrap", marginBottom: 8 }}
      >
        <label className="search grow">
          <Search size={14} className="faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search the activity"
            aria-label="Search the activity"
          />
        </label>
        <button className="icon-btn" onClick={load} title="Refresh">
          <RefreshCw size={14} />
        </button>
        <button
          className="btn sm ghost"
          onClick={() => void copy()}
          disabled={!list.length}
        >
          {copied ? <Check size={13} /> : <Copy size={13} />} Copy as JSON
        </button>
        <button
          className="btn sm ghost"
          onClick={() => void clear()}
          disabled={!list.length}
        >
          <Trash2 size={13} /> Clear
        </button>
      </div>
      <div className="chips" style={{ marginBottom: 8 }}>
        <button
          className="chip"
          aria-pressed={kind === null}
          onClick={() => setKind(null)}
        >
          All
        </button>
        {KINDS.map((k) => (
          <button
            key={k.id}
            className="chip"
            aria-pressed={kind === k.id}
            onClick={() => setKind(k.id)}
          >
            {k.label}
          </button>
        ))}
      </div>
      {error && <div className="banner danger">{error}</div>}
      {!list.length && (
        <p className="faint small">
          Nothing here{query || kind ? " that matches" : " yet"}.
        </p>
      )}
      <div className="activity-log">
        {groups.map((g) => (
          <div key={g.day}>
            <h4>{g.day}</h4>
            {g.items.map((a) => {
              const i = n++;
              const detail = argsLine(a.args);
              return (
                <div key={i} className={`activity-row ${a.ok ? "" : "failed"}`}>
                  <button
                    className="activity-main"
                    onClick={() => setOpen(open === i ? null : i)}
                    aria-expanded={open === i}
                  >
                    <span className="activity-time faint small">
                      {new Date(a.ts).toLocaleTimeString(undefined, {
                        hour: "numeric",
                        minute: "2-digit",
                      })}
                    </span>
                    <span className={`activity-kind kind-${a.kind}`}>
                      {kindLabel(a.kind)}
                    </span>
                    <span className="grow">
                      {toolLabel(a.tool)}
                      {detail && <span className="faint"> · {detail}</span>}
                    </span>
                    {!a.ok && (
                      <span className="perm-status denied">Didn't work</span>
                    )}
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

function KidsSection() {
  const [pin, setPin] = useState("");
  const [again, setAgain] = useState("");
  const [error, setError] = useState<string | null>(null);
  const turnOn = async () => {
    setError(null);
    if (pin !== again) return setError("The two PINs don't match.");
    try {
      const settings = await api.kidsEnter(pin);
      useStore.setState({ settings });
      useStore.getState().openSettings(null);
      // Kids mode shows only the kids' own chats.
      await useStore.getState().reloadChats();
      useStore.getState().newChat();
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <section className="settings-section">
      <h3>
        <Baby size={16} /> Kids mode
      </h3>
      <p className="faint small">
        A simple BYTE for children: answers written for kids, bigger text, and
        no web, Mac control, files, terminal or connectors. Settings can't be
        changed while it's on. A grown-up turns it off with the PIN (the
        Grown-ups button at the top).
      </p>
      <div
        className="row"
        style={{ gap: 8, flexWrap: "wrap", alignItems: "center" }}
      >
        <input
          className="text-input"
          type="password"
          inputMode="numeric"
          maxLength={6}
          placeholder="PIN (4–6 digits)"
          value={pin}
          onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
          aria-label="Kids mode PIN"
        />
        <input
          className="text-input"
          type="password"
          inputMode="numeric"
          maxLength={6}
          placeholder="PIN again"
          value={again}
          onChange={(e) => setAgain(e.target.value.replace(/\D/g, ""))}
          aria-label="Kids mode PIN again"
        />
        <button
          className="btn sm primary"
          disabled={pin.length < 4 || again.length < 4}
          onClick={() => void turnOn()}
        >
          Turn on kids mode
        </button>
      </div>
      {error && <div className="banner danger">{error}</div>}
    </section>
  );
}

function BackupSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [info, setInfo] = useState<BackupInfo | null>(null);
  const [pass, setPass] = useState("");
  const [remember, setRemember] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [restoring, setRestoring] = useState<string | null>(null);
  const [restorePass, setRestorePass] = useState("");

  const load = useCallback(() => {
    if (inTauri)
      void api.backupInfo().then(setInfo, (e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load, settings?.backupDir]);

  const now = async () => {
    setBusy("backup");
    setError(null);
    setNote(null);
    try {
      const path = await api.backupNow(pass || null, remember);
      setNote(`Saved ${path.split("/").pop()}`);
      setPass("");
      load();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };
  const pickRestore = async () => {
    const picked = await openDialog({
      title: "Choose a BYTE backup",
      filters: [{ name: "BYTE backup", extensions: ["bytebackup"] }],
      defaultPath: info?.dir,
    }).catch(() => null);
    if (typeof picked === "string") setRestoring(picked);
  };
  const restore = async () => {
    if (!restoring) return;
    setBusy("restore");
    setError(null);
    try {
      await api.backupRestore(restoring, restorePass);
    } catch (e) {
      setError(errorText(e));
      setBusy(null);
    }
  };

  return (
    <section className="settings-section">
      <h3>
        <Archive size={16} /> Backups
      </h3>
      <p className="faint small">
        One encrypted file with your chats, memories, settings and notes
        {info?.icloud
          ? ", kept in iCloud Drive so it's safe if this Mac is lost"
          : ""}
        . Only your passphrase opens it, so don't forget it. Models aren't
        included (they can be downloaded again).
      </p>
      <div
        className="row"
        style={{ gap: 8, flexWrap: "wrap", alignItems: "center" }}
      >
        {info?.icloud ? <Cloud size={14} className="faint" /> : null}
        <span className="small grow" title={info?.dir}>
          {info ? info.dir.replace(/^\/Users\/[^/]+/, "~") : "…"}
        </span>
        <button
          className="btn sm ghost"
          onClick={async () => {
            const d = await openDialog({
              directory: true,
              title: "Where to keep backups",
            }).catch(() => null);
            if (typeof d === "string") await update({ backupDir: d });
          }}
        >
          Change folder…
        </button>
        {settings?.backupDir && (
          <button
            className="btn sm ghost"
            onClick={() => void update({ backupDir: null })}
          >
            Use the default
          </button>
        )}
      </div>
      <div
        className="row"
        style={{ gap: 8, flexWrap: "wrap", alignItems: "center", marginTop: 8 }}
      >
        <input
          className="text-input grow"
          type="password"
          placeholder={
            info?.remembered
              ? "Passphrase (saved in your Keychain; type to change)"
              : "Passphrase (at least 8 characters)"
          }
          value={pass}
          onChange={(e) => setPass(e.target.value)}
          aria-label="Backup passphrase"
        />
        <label className="row small" style={{ gap: 6 }}>
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
          />{" "}
          Remember in Keychain
        </label>
        <button
          className="btn sm primary"
          disabled={!!busy || (!pass && !info?.remembered)}
          onClick={() => void now()}
        >
          {busy === "backup" ? "Backing up…" : "Back up now"}
        </button>
      </div>
      <div className="field" style={{ display: "block", marginTop: 8 }}>
        <label className="row" style={{ alignItems: "flex-start", gap: 12 }}>
          <span className="grow">
            Back up every week
            <small>
              Uses the passphrase saved in your Keychain. The newest 5 backups
              are kept.
              {settings?.lastBackup
                ? ` Last backup: ${backupAge(settings.lastBackup)}.`
                : ""}
            </small>
          </span>
          <input
            type="checkbox"
            checked={!!settings?.backupAuto}
            disabled={!info?.remembered && !settings?.backupAuto}
            onChange={(e) => void update({ backupAuto: e.target.checked })}
            aria-label="Back up every week"
          />
        </label>
        <label className="row" style={{ alignItems: "flex-start", gap: 12 }}>
          <span className="grow">
            Include my notes
            <small>Your notes folder goes into the backup too.</small>
          </span>
          <input
            type="checkbox"
            checked={settings?.backupIncludeNotes !== false}
            onChange={(e) =>
              void update({ backupIncludeNotes: e.target.checked })
            }
            aria-label="Include my notes"
          />
        </label>
      </div>
      {info && info.files.length > 0 && (
        <ul className="backup-list">
          {info.files.map((f) => (
            <li key={f.path}>
              <span className="grow small">
                {f.name.replace(/\.bytebackup$/, "")}
              </span>
              <span className="faint small">{formatBytes(f.size)}</span>
              <button
                className="btn sm ghost"
                onClick={() => setRestoring(f.path)}
              >
                Restore…
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="row" style={{ gap: 8 }}>
        <button className="btn sm ghost" onClick={() => void pickRestore()}>
          Restore from a file…
        </button>
        {info?.remembered && (
          <button
            className="btn sm ghost"
            onClick={() => void api.backupForget().then(load)}
          >
            Forget the saved passphrase
          </button>
        )}
      </div>
      {restoring && (
        <div className="banner warn" style={{ marginTop: 8 }}>
          <span className="grow small">
            Restore <b>{restoring.split("/").pop()}</b>? Your current chats and
            settings are set aside (kept in BYTE's data folder) and BYTE
            restarts.
          </span>
          <input
            className="text-input"
            type="password"
            placeholder="Its passphrase"
            value={restorePass}
            onChange={(e) => setRestorePass(e.target.value)}
            aria-label="Passphrase of the backup"
          />
          <button
            className="btn sm primary"
            disabled={!restorePass || !!busy}
            onClick={() => void restore()}
          >
            {busy === "restore" ? "Restoring…" : "Restore and restart"}
          </button>
          <button
            className="icon-btn"
            onClick={() => setRestoring(null)}
            aria-label="Cancel"
          >
            <X size={14} />
          </button>
        </div>
      )}
      {note && <p className="small">{note}</p>}
      {error && <div className="banner danger">{error}</div>}
    </section>
  );
}

const KEEP_CHOICES = [0, 30, 90, 180, 365];

function OldChatsSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  return (
    <section className="settings-section">
      <h3>
        <Trash2 size={16} /> Old chats
      </h3>
      <div className="row" style={{ gap: 12, alignItems: "center" }}>
        <span className="grow small">
          Delete chats you haven't opened in a while. Pinned chats are always
          kept. Checked once a day.
        </span>
        <select
          value={settings?.autoDeleteDays ?? 0}
          onChange={(e) =>
            void update({ autoDeleteDays: Number(e.target.value) })
          }
          aria-label="Delete old chats after"
        >
          {KEEP_CHOICES.map((d) => (
            <option key={d} value={d}>
              {d === 0 ? "Keep them all" : `After ${d} days`}
            </option>
          ))}
        </select>
      </div>
    </section>
  );
}

function EraseSection() {
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <section className="settings-section danger-zone">
      <h3>
        <AlertTriangle size={16} /> Erase everything
      </h3>
      <p className="small">
        Deletes all chats, memories, settings, boards and the activity log in
        this profile, and restarts BYTE as new. Downloaded models stay, and so
        do your notes files in Documents. This can't be undone (a backup can
        bring it back).
      </p>
      <div className="row" style={{ gap: 8, alignItems: "center" }}>
        <input
          className="text-input"
          placeholder="Type ERASE"
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          aria-label="Type ERASE to confirm"
        />
        <button
          className="btn sm danger"
          disabled={typed.trim() !== "ERASE" || busy}
          onClick={async () => {
            setBusy(true);
            setError(null);
            try {
              await api.eraseEverything(typed);
            } catch (e) {
              setError(errorText(e));
              setBusy(false);
            }
          }}
        >
          Erase everything
        </button>
      </div>
      {error && <div className="banner danger">{error}</div>}
    </section>
  );
}

const FULL_DISK =
  "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AllFiles";

/** The Messages inbox: texts you receive, with replies BYTE can draft (needs Full Disk Access). */
function MessagesSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [status, setStatus] = useState<MessagesStatus | null>(null);
  const check = useCallback(() => {
    if (inTauri) api.messagesStatus().then(setStatus, () => undefined);
  }, []);
  useEffect(check, [check, settings?.messagesInbox]);
  if (status && !status.available) return null;
  const on = settings?.messagesInbox === true;
  return (
    <section className="settings-section">
      <h3>
        <MessageCircle size={16} /> Messages inbox
      </h3>
      <div className="field" style={{ display: "block" }}>
        <label className="row" style={{ alignItems: "flex-start", gap: 12 }}>
          <span className="grow">
            Show texts I receive
            <small>
              New iMessages and texts show up in BYTE (the speech-bubble button)
              with Draft a reply and Reply. BYTE reads your Messages history on
              this Mac, read-only; nothing leaves your Mac. Needs Full Disk
              Access. Sending always waits for you to press Send.
            </small>
          </span>
          <input
            type="checkbox"
            checked={on}
            onChange={(e) => void update({ messagesInbox: e.target.checked })}
            aria-label="Show texts I receive"
          />
        </label>
      </div>
      {on && (
        <>
          {status?.granted ? (
            <p className="ok small">
              <Check size={13} /> BYTE can read your messages.
            </p>
          ) : (
            <div className="banner warn">
              <span className="grow">{status?.message ?? "Checking…"}</span>
              <button
                className="btn sm"
                onClick={() => void api.upkeepOpenSettings(FULL_DISK)}
              >
                <ExternalLink size={13} /> Open Full Disk Access
              </button>
              <button className="btn sm ghost" onClick={check}>
                <RefreshCw size={13} /> Check again
              </button>
            </div>
          )}
          <div className="field" style={{ display: "block" }}>
            <label className="row" style={{ gap: 12 }}>
              <span className="grow">
                Notify me when a text arrives
                <small>
                  A notification and a banner in BYTE. While BYTE is locked it
                  shows who texted, not what they said.
                </small>
              </span>
              <input
                type="checkbox"
                checked={settings?.messagesNotify !== false}
                onChange={(e) =>
                  void update({ messagesNotify: e.target.checked })
                }
                aria-label="Notify me when a text arrives"
              />
            </label>
          </div>
        </>
      )}
    </section>
  );
}
