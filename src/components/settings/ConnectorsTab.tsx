import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { CalendarDays, FolderOpen, NotebookPen, Plus, Trash2, Unplug } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import type { ConnectorsStatus } from "../../lib/types";
import { useStore } from "../../state/store";
import { shortPath } from "./KnowledgeTab";

/** Obsidian, Notion and calendar links (Rust `connectors/`). Secrets go to the macOS Keychain, never to settings. */
export function ConnectorsTab() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [st, setSt] = useState<ConnectorsStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [secret, setSecret] = useState("");
  const [parent, setParent] = useState("");
  const [calName, setCalName] = useState("");
  const [calUrl, setCalUrl] = useState("");

  useEffect(() => {
    api.connectorsStatus().then(setSt, (e) => setError(errorText(e)));
  }, []);

  const guard = async (fn: () => Promise<ConnectorsStatus | void>, done?: string) => {
    setError(null);
    setNotice(null);
    setBusy(true);
    try {
      const next = await fn();
      if (next) setSt(next);
      if (done) setNotice(done);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  if (!settings) return null;
  const on = settings.connectorsEnabled !== false;

  return (
    <>
      <h3>Connectors</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        Let BYTE use your notes and calendars. Each one is off until you set it up here. Secrets stay in your Mac's Keychain, and BYTE only reads or adds things when you ask.
      </p>
      {error && <div className="banner danger">{error}</div>}
      {notice && <div className="banner">{notice}</div>}
      {st && !st.keychain && <div className="banner">Notion and calendar links keep their secrets in the macOS Keychain, so they need a Mac for now.</div>}

      <div className="section">
        <label className="row" style={{ gap: 10, cursor: "pointer" }}>
          <input type="checkbox" checked={on} onChange={(e) => void update({ connectorsEnabled: e.target.checked })} />
          <span>
            <b>Use connectors in chat</b>
            <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
              Off: BYTE doesn't read or add to any of them, even if they're set up.
            </span>
          </span>
        </label>
      </div>

      <div className="section connector">
        <h4>
          <NotebookPen size={15} /> Obsidian
        </h4>
        {st?.vault ? (
          <>
            <p className="muted small">
              Vault: <b>{shortPath(st.vault)}</b> · {st.vaultNotes} notes. Ask “what do my Obsidian notes say about…”, or “save that to Obsidian” (new notes go in the vault's BYTE folder, after your OK, with Undo).
            </p>
            <button className="btn sm ghost" disabled={busy} onClick={() => void guard(() => api.obsidianSet(null), "Obsidian disconnected. Your vault wasn't changed.")}>
              <Unplug size={13} /> Disconnect
            </button>
          </>
        ) : (
          <>
            <p className="muted small">Pick your vault folder (the one with your notes). BYTE searches it and can add new notes.</p>
            <button
              className="btn sm primary"
              disabled={busy}
              onClick={() =>
                void guard(async () => {
                  const dir = await openDialog({ directory: true, title: "Choose your Obsidian vault" });
                  if (typeof dir === "string") return api.obsidianSet(dir);
                })
              }
            >
              <FolderOpen size={13} /> Choose vault…
            </button>
          </>
        )}
      </div>

      <div className="section connector">
        <h4>
          <NotebookPen size={15} /> Notion
        </h4>
        {st?.notion ? (
          <>
            <p className="muted small">
              Connected. Ask “search Notion for…”, or “add a page to Notion: Title — text”. BYTE sees only pages you've shared with your integration (in Notion: ••• → Connections).
              {!st.notionParent && " To add pages, connect again with the link of the page they should go under."}
            </p>
            <button className="btn sm ghost" disabled={busy} onClick={() => void guard(() => api.notionDisconnect(), "Notion disconnected; the secret was removed from the Keychain.")}>
              <Unplug size={13} /> Disconnect
            </button>
          </>
        ) : (
          <>
            <ol className="muted small connector-steps">
              <li>
                Go to <b>notion.so/my-integrations</b> → New integration (internal) → copy its secret.
              </li>
              <li>In Notion, open the pages BYTE may use → ••• → Connections → add your integration.</li>
            </ol>
            <div className="auto-row">
              <input className="grow" type="password" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder="Integration secret (ntn_…)" aria-label="Notion secret" autoComplete="off" />
            </div>
            <div className="auto-row">
              <input className="grow" value={parent} onChange={(e) => setParent(e.target.value)} placeholder="Add new pages under (a Notion page link, optional)" aria-label="Notion parent page" />
              <button
                className="btn sm primary"
                disabled={busy || secret.trim().length < 20}
                onClick={() =>
                  void guard(async () => {
                    const next = await api.notionConnect(secret, parent || null);
                    setSecret("");
                    setParent("");
                    return next;
                  }, "Notion is connected. The secret is in your Keychain.")
                }
              >
                Connect
              </button>
            </div>
          </>
        )}
      </div>

      <div className="section connector">
        <h4>
          <CalendarDays size={15} /> Calendar links
        </h4>
        <p className="muted small">
          Read-only calendars from a private address, like Google Calendar's <b>Secret address in iCal format</b> (Settings → your calendar → Integrate calendar). They show in your daily briefing and, when Mac control is off, in “what's on my calendar”. The address is kept in the Keychain.
        </p>
        {st && st.calendars.length > 0 && (
          <ul className="schedule-list">
            {st.calendars.map(([name, host], i) => (
              <li key={`${name}-${i}`}>
                <CalendarDays size={15} />
                <div className="grow">
                  <b>{name}</b>
                  <div className="muted small">{host}</div>
                </div>
                <button className="icon-btn sm" disabled={busy} onClick={() => void guard(() => api.calendarLinkRemove(i))} aria-label={`Remove ${name}`} title="Remove">
                  <Trash2 size={13} />
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="auto-row">
          <input value={calName} onChange={(e) => setCalName(e.target.value)} placeholder="Name (Work)" aria-label="Calendar name" />
          <input className="grow" type="password" value={calUrl} onChange={(e) => setCalUrl(e.target.value)} placeholder="https://… or webcal://… (.ics)" aria-label="Calendar address" autoComplete="off" />
          <button
            className="btn sm primary"
            disabled={busy || !calUrl.trim()}
            onClick={() =>
              void guard(async () => {
                const [next, count] = await api.calendarLinkAdd(calName, calUrl);
                setCalName("");
                setCalUrl("");
                setNotice(`Added: BYTE read ${count} events from it.`);
                return next;
              })
            }
          >
            <Plus size={13} /> Add
          </button>
        </div>
      </div>
      <p className="faint small">Google (Gmail, Drive, Calendar), Dropbox, OneDrive and Spotify come later: they need BYTE registered with each company first.</p>
    </>
  );
}
