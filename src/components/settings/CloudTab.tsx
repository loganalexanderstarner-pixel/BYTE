import { CloudKeySteps } from "./CloudKeySteps";
import { onDevice } from "../../lib/device";
import { ask } from "@tauri-apps/plugin-dialog";
import { Cloud, Download, KeyRound, LogOut, RefreshCw } from "lucide-react";
import { useState } from "react";

import { api, errorText } from "../../lib/api";
import { budgetRows, listOf } from "../../lib/cloudDocs";
import { CloudAccount } from "./CloudAccount";
import { useStore, workspaceOf } from "../../state/store";

export function CloudTab() {
  const settings = useStore((s) => s.settings);
  const cloud = useStore((s) => s.cloud);
  const refreshCloud = useStore((s) => s.refreshCloud);
  const setWorkspace = useStore((s) => s.setWorkspace);
  const reloadChats = useStore((s) => s.reloadChats);
  const [key, setKey] = useState("");
  const [address, setAddress] = useState("");
  const [showAddress, setShowAddress] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const reloadSettings = async () =>
    useStore.setState({ settings: await api.settingsGet() });
  const run = async (label: string, fn: () => Promise<void>) => {
    setError(null);
    setNote(null);
    setBusy(label);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const connect = () =>
    run("connect", async () => {
      await api.cloudConnect(key, address.trim() || null);
      setKey(""); // never kept in the page once it's in the Keychain
      await Promise.all([reloadSettings(), refreshCloud()]);
      await setWorkspace("cloud");
    });

  const disconnect = () =>
    run("disconnect", async () => {
      const yes = await ask(
        onDevice("Remove your BYTE cloud key from this Mac? Chats already here stay."),
        { title: "Disconnect BYTE Cloud", kind: "warning" },
      ).catch(() => true);
      if (!yes) return;
      await api.cloudDisconnect();
      await Promise.all([reloadSettings(), refreshCloud()]);
    });

  const refresh = () =>
    run("refresh", async () => {
      try {
        await api.cloudRefresh();
      } finally {
        await Promise.all([reloadSettings(), refreshCloud()]);
      }
    });

  const importChats = () =>
    run("import", async () => {
      const list = listOf(await api.cloudConversations());
      let n = 0;
      for (const c of list) {
        const id = c.id ?? c.conversation_id;
        if (id === undefined || id === null) continue;
        await api.cloudImport(String(id));
        n += 1;
        setNote(`Importing… ${n} of ${list.length}`);
      }
      await reloadChats();
      setNote(
        n
          ? `Imported ${n} cloud chat${n === 1 ? "" : "s"}. They're in the sidebar.`
          : "No chats on your BYTE cloud yet.",
      );
    });

  if (!settings) return null;
  const me = cloud?.account;
  const connected = !!cloud?.connected || settings.cloudConnected;
  const budgets = budgetRows(me?.budgets);

  return (
    <>
      <h3>Cloud</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        Use your own BYTE cluster when this Mac is too small or you want its
        bigger models. Answers stream from the cloud; if it can't be reached,
        BYTE answers on this Mac instead. Private chats never leave this Mac.
      </p>
      {error && <div className="banner danger">{error}</div>}
      {note && <div className="banner">{note}</div>}

      {!connected ? (
        <div className="section">
          <h4>Connect</h4>
          <div className="field">
            <label>
              <span className="row" style={{ gap: 6 }}>
                <KeyRound size={14} style={{ color: "var(--accent)" }} /> API
                key
              </span>
            </label>
          </div>
          <CloudKeySteps />
          <div className="row" style={{ gap: 8 }}>
            <input
              className="text-input key-input"
              type="password"
              autoComplete="off"
              spellCheck={false}
              placeholder="byte_…"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              onKeyDown={(e) =>
                e.key === "Enter" && key.trim() && void connect()
              }
              aria-label="BYTE cloud API key"
            />
            <button
              className="btn primary"
              disabled={!key.trim() || !!busy}
              onClick={() => void connect()}
            >
              <Cloud size={14} /> {busy === "connect" ? "Checking…" : "Connect"}
            </button>
          </div>
          <button
            className="btn sm ghost"
            style={{ marginTop: 8 }}
            onClick={() => setShowAddress(!showAddress)}
          >
            {showAddress ? "Hide address" : "Use a different address"}
          </button>
          {showAddress && (
            <input
              className="text-input"
              style={{ marginTop: 8, width: "100%" }}
              placeholder={cloud?.baseUrl ?? "https://byteai.bytebylogan.xyz"}
              value={address}
              onChange={(e) => setAddress(e.target.value)}
              aria-label="Cloud address"
            />
          )}
        </div>
      ) : (
        <>
          <div className="cloud-account">
            <b>
              <Cloud
                size={14}
                style={{ verticalAlign: -2, color: "var(--accent)" }}
              />{" "}
              {me?.name ?? me?.email ?? "Connected"}
              {me?.tier && <span className="model-label">{me.tier}</span>}
            </b>
            {me?.email && me.name && <span className="faint">{me.email}</span>}
            <span className="faint">{cloud?.baseUrl}</span>
            {me && me.modes.length > 0 && (
              <div className="cloud-modes">
                {me.modes.map((m) => (
                  <span key={m.id} className="pill">
                    {m.label}
                  </span>
                ))}
              </div>
            )}
            {budgets.length > 0 && (
              <div className="faint" style={{ fontSize: "0.85em" }}>
                {budgets.map(([k, v]) => (
                  <div key={k}>
                    {k}: {v}
                  </div>
                ))}
              </div>
            )}
          </div>
          <div className="section">
            <div className="field">
              <label>
                Where BYTE answers
                <small>
                  Also switchable at the top of the sidebar. Both asks this Mac
                  and the cloud at once so you can keep the better answer.
                </small>
              </label>
              <div
                className="segmented"
                role="tablist"
                aria-label="Where BYTE answers"
              >
                {(["local", "cloud", "both"] as const).map((ws) => (
                  <button
                    key={ws}
                    role="tab"
                    aria-selected={workspaceOf(settings) === ws}
                    onClick={() => void setWorkspace(ws)}
                  >
                    {ws === "local"
                      ? onDevice("This Mac")
                      : ws === "cloud"
                        ? "Cloud"
                        : "Both"}
                  </button>
                ))}
              </div>
            </div>
            <div className="field">
              <label>
                Cloud chats
                <small>
                  Copy the conversations from your BYTE cloud into this Mac's
                  sidebar (and search).
                </small>
              </label>
              <button
                className="btn sm"
                disabled={!!busy}
                onClick={() => void importChats()}
              >
                <Download size={14} />{" "}
                {busy === "import" ? "Importing…" : "Import"}
              </button>
            </div>
            <div className="field">
              <label>
                Account
                <small>
                  Refresh after changing your plan. Disconnecting removes the
                  key from this Mac's Keychain.
                </small>
              </label>
              <div className="row" style={{ gap: 6 }}>
                <button
                  className="btn sm"
                  disabled={!!busy}
                  onClick={() => void refresh()}
                >
                  <RefreshCw
                    size={14}
                    className={busy === "refresh" ? "spin" : undefined}
                  />{" "}
                  Refresh
                </button>
                <button
                  className="btn sm danger"
                  disabled={!!busy}
                  onClick={() => void disconnect()}
                >
                  <LogOut size={14} /> Disconnect
                </button>
              </div>
            </div>
          </div>
          <CloudAccount />
        </>
      )}
    </>
  );
}
