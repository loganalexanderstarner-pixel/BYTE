import { documentDir, join } from "@tauri-apps/api/path";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Download, FileUp, Loader2, Plus, RefreshCw, Search, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { bodyOf, idOf, listOf, str, titleOf, type Row } from "../../lib/cloudDocs";
import { useStore } from "../../state/store";
import { osText } from "../../lib/platform";

type Section = "memories" | "knowledge" | "prompts" | "recipes" | "context" | "search" | "export";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "memories", label: "Memories" },
  { id: "knowledge", label: "Knowledge" },
  { id: "prompts", label: "Saved prompts" },
  { id: "recipes", label: "Recipes" },
  { id: "context", label: "About you" },
  { id: "search", label: "Search" },
  { id: "export", label: "Export" },
];

/** Everything else the web app keeps for this account, managed from the Mac. */
export function CloudAccount() {
  const [section, setSection] = useState<Section>("memories");
  return (
    <div className="section cloud-account-data">
      <h4>Your cloud account</h4>
      <div className="doc-kinds" role="tablist" aria-label="Cloud account">
        {SECTIONS.map((s) => (
          <button key={s.id} role="tab" aria-selected={section === s.id} className={`pill ${section === s.id ? "accent" : ""}`} onClick={() => setSection(s.id)}>
            {s.label}
          </button>
        ))}
      </div>
      {section === "memories" && (
        <Collection path="/api/memories" noun="memory" hint={osText("What BYTE remembers about you on the cloud (separate from memories kept on this Mac).")} fields={["text"]} />
      )}
      {section === "knowledge" && (
        <Collection path="/api/knowledge" noun="reference" hint="Reference material the cloud can draw on when answering." fields={["title", "text"]} upload />
      )}
      {section === "prompts" && (
        <Collection path="/api/saved-prompts" noun="prompt" hint="Type / in the chat box to use one." fields={["title", "text"]} onChange={() => void useStore.getState().loadSavedPrompts(true)} />
      )}
      {section === "recipes" && <Collection path="/api/recipes" noun="recipe" hint="Recipes saved from your chats." fields={["title", "text"]} />}
      {section === "context" && <PersonalContext />}
      {section === "search" && <CloudSearch />}
      {section === "export" && <Export />}
    </div>
  );
}

function Collection({ path, noun, hint, fields, upload, onChange }: { path: string; noun: string; hint: string; fields: ("title" | "text")[]; upload?: boolean; onChange?(): void }) {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    setError(null);
    api
      .cloudGet(path)
      .then((v) => setRows(listOf(v)))
      .catch((e) => setError(errorText(e)));
  }, [path]);
  useEffect(load, [load]);

  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      load();
      onChange?.();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const add = () =>
    act(async () => {
      // The same text under the names the web app uses for it.
      const body: Row = { content: text.trim(), text: text.trim() };
      if (fields.includes("title")) Object.assign(body, { title: title.trim(), name: title.trim() });
      if (path.includes("prompt")) body.prompt = text.trim();
      await api.cloudPost(path, body);
      setTitle("");
      setText("");
    });

  const uploadFile = async () => {
    const file = await openDialog({ title: `Add ${noun}`, filters: [{ name: "Documents", extensions: ["pdf", "docx", "pptx", "txt", "md", "csv"] }] });
    if (typeof file === "string") await act(() => api.cloudUpload(path, file));
  };

  return (
    <div className="collection">
      <p className="faint" style={{ marginTop: 0 }}>
        {hint}
      </p>
      {error && <div className="banner danger">{error}</div>}
      <div className="collection-add">
        {fields.includes("title") && (
          <input className="text-input" placeholder="Title" value={title} onChange={(e) => setTitle(e.target.value)} aria-label={`New ${noun} title`} />
        )}
        <textarea className="text-input" rows={2} placeholder={`New ${noun}…`} value={text} onChange={(e) => setText(e.target.value)} aria-label={`New ${noun}`} />
        <div className="row" style={{ gap: 6 }}>
          <button className="btn sm primary" disabled={!text.trim() || busy} onClick={() => void add()}>
            <Plus size={14} /> Add
          </button>
          {upload && (
            <button className="btn sm" disabled={busy} onClick={() => void uploadFile()}>
              <FileUp size={14} /> Upload a file
            </button>
          )}
          <span className="spacer" />
          <button className="icon-btn" onClick={load} title="Refresh" aria-label="Refresh">
            <RefreshCw size={14} />
          </button>
        </div>
      </div>
      {!rows && !error && <Loader2 className="spin" size={16} />}
      {rows?.length === 0 && <p className="faint">Nothing here yet.</p>}
      <ul className="collection-list">
        {rows?.map((r) => {
          const id = idOf(r);
          const body = bodyOf(r);
          const t = str(r.title) ?? str(r.name) ?? str(r.filename);
          return (
            <li key={id ?? body}>
              <div className="grow">
                {t && <b>{t}</b>}
                {body && <div className="faint clamp">{body}</div>}
              </div>
              {id && (
                <button className="icon-btn" disabled={busy} onClick={() => void act(() => api.cloudDelete(`${path}/${id}`))} aria-label={`Delete ${t ?? noun}`}>
                  <Trash2 size={14} />
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function PersonalContext() {
  const modes = useStore((s) => s.cloud?.account?.modes ?? []);
  const [settings, setSettings] = useState<Row | null>(null);
  const [context, setContext] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .cloudGet<Row>("/api/settings")
      .then((s) => {
        setSettings(s ?? {});
        setContext(str(s?.personal_context) ?? str(s?.personalContext) ?? str((s?.personal_context as Row | undefined)?.text) ?? "");
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  const saveContext = async () => {
    setError(null);
    try {
      await api.cloudPost("/api/settings/personal-context", { personal_context: context, context, text: context });
      setNote("Saved on your cloud.");
    } catch (e) {
      setError(errorText(e));
    }
  };

  const setDefaultMode = async (mode: string) => {
    setError(null);
    try {
      await api.cloudPost("/api/settings", { ...(settings ?? {}), default_mode: mode });
      setSettings({ ...(settings ?? {}), default_mode: mode });
      setNote("Default mode saved on your cloud.");
    } catch (e) {
      setError(errorText(e));
    }
  };

  const defaultMode = str(settings?.default_mode) ?? str(settings?.defaultMode) ?? "";
  return (
    <div className="collection">
      {error && <div className="banner danger">{error}</div>}
      {note && <div className="banner">{note}</div>}
      <p className="faint" style={{ marginTop: 0 }}>
        What the cloud should know about you in every chat (your web app's personal context).
      </p>
      <textarea className="text-input" rows={4} value={context} onChange={(e) => setContext(e.target.value)} aria-label="Personal context" />
      <div className="row" style={{ gap: 6, marginTop: 6 }}>
        <button className="btn sm primary" onClick={() => void saveContext()}>
          Save
        </button>
      </div>
      {modes.length > 0 && (
        <div className="field" style={{ marginTop: 12 }}>
          <label>
            Default mode on the web app
            <small>Only modes your plan includes are listed.</small>
          </label>
          <select value={defaultMode} onChange={(e) => void setDefaultMode(e.target.value)} aria-label="Default cloud mode">
            {!defaultMode && <option value="">—</option>}
            {modes.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label}
              </option>
            ))}
          </select>
        </div>
      )}
    </div>
  );
}

function CloudSearch() {
  const [q, setQ] = useState("");
  const [rows, setRows] = useState<Row[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reloadChats = useStore((s) => s.reloadChats);
  const selectChat = useStore((s) => s.selectChat);
  const openSettings = useStore((s) => s.openSettings);

  const search = async () => {
    setError(null);
    try {
      setRows(listOf(await api.cloudGet(`/api/search/conversations?q=${encodeURIComponent(q.trim())}`)));
    } catch (e) {
      setError(errorText(e));
    }
  };

  const open = async (r: Row) => {
    const cid = str(r.conversation_id) ?? idOf(r);
    if (!cid) return;
    try {
      const local = await api.cloudImport(cid);
      await reloadChats();
      await selectChat(local);
      openSettings(null);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="collection">
      <div className="row" style={{ gap: 6 }}>
        <input
          className="text-input"
          style={{ flex: 1 }}
          placeholder="Search your cloud chats"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && q.trim() && void search()}
          aria-label="Search cloud chats"
        />
        <button className="btn sm" disabled={!q.trim()} onClick={() => void search()}>
          <Search size={14} /> Search
        </button>
      </div>
      {error && <div className="banner danger">{error}</div>}
      {rows?.length === 0 && <p className="faint">No matches.</p>}
      <ul className="collection-list">
        {rows?.map((r, i) => (
          <li key={idOf(r) ?? i}>
            <button className="linklike grow" onClick={() => void open(r)}>
              <b>{titleOf(r)}</b>
              {(str(r.snippet) ?? bodyOf(r)) && <div className="faint clamp">{str(r.snippet) ?? bodyOf(r)}</div>}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Export() {
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setError(null);
    try {
      const dest = await saveDialog({ defaultPath: await join(await documentDir(), "BYTE", `byte-cloud-export-${new Date().toISOString().slice(0, 10)}.zip`) });
      if (!dest) return;
      setBusy(true);
      await api.cloudDownload("/api/export", dest);
      setSaved(dest);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="collection">
      <p className="faint" style={{ marginTop: 0 }}>
        Everything on your cloud account (chats, documents, memories and more) as one .zip file.
      </p>
      {error && <div className="banner danger">{error}</div>}
      {saved && (
        <div className="banner">
          <span className="grow">Saved to {saved}</span>
          <button className="btn sm" onClick={() => void revealItemInDir(saved)}>
            {osText("Show in Finder")}
          </button>
        </div>
      )}
      <button className="btn sm primary" disabled={busy} onClick={() => void run()}>
        {busy ? <Loader2 size={14} className="spin" /> : <Download size={14} />} Export everything
      </button>
    </div>
  );
}
