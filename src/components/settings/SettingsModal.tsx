import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
import { Brain, Cpu, Download, HardDrive, Info, Palette, Plus, RefreshCw, ShieldCheck, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";

import { THEMES } from "../../design/themes";
import { Logo } from "../../design/Logo";
import { api, errorText } from "../../lib/api";
import { bytes, contextLabel, ramSize } from "../../lib/format";
import { displayName } from "../../lib/models";
import type { Memory } from "../../lib/types";
import { useStore, type SettingsTab } from "../../state/store";
import { CatalogBrowser } from "../models/CatalogBrowser";

const TABS: { id: SettingsTab; label: string; icon: typeof Cpu }[] = [
  { id: "models", label: "Models", icon: HardDrive },
  { id: "memory", label: "Memory & chats", icon: Brain },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "engine", label: "Engine", icon: Cpu },
  { id: "about", label: "About", icon: Info },
];

export function SettingsModal() {
  const tab = useStore((s) => s.settingsTab)!;
  const openSettings = useStore((s) => s.openSettings);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && openSettings(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openSettings]);

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && openSettings(null)}>
      <div className="modal" role="dialog" aria-modal="true" aria-label="Settings">
        <nav className="modal-nav">
          <h2>Settings</h2>
          {TABS.map(({ id, label, icon: Icon }) => (
            <button key={id} aria-current={tab === id} onClick={() => openSettings(id)}>
              <Icon size={16} />
              {label}
            </button>
          ))}
        </nav>
        <section className="modal-body">
          <button className="icon-btn modal-close" onClick={() => openSettings(null)} aria-label="Close settings">
            <X size={18} />
          </button>
          {tab === "models" && <ModelsTab />}
          {tab === "memory" && <MemoryTab />}
          {tab === "appearance" && <AppearanceTab />}
          {tab === "engine" && <EngineTab />}
          {tab === "about" && <AboutTab />}
        </section>
      </div>
    </div>
  );
}

function ModelsTab() {
  return (
    <>
      <h3>Models</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        Pick the brain BYTE runs on. Everything runs on this Mac's GPU; files download from Hugging Face only when you choose them.
      </p>
      <CatalogBrowser />
    </>
  );
}

function MemoryTab() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const reloadChats = useStore((s) => s.reloadChats);
  const [memories, setMemories] = useState<Memory[]>([]);
  const [about, setAbout] = useState(settings?.aboutMe ?? "");
  const [draft, setDraft] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => api.memoriesList().then(setMemories).catch((e) => setError(errorText(e)));
  useEffect(() => {
    void refresh();
  }, []);

  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
  };

  if (!settings) return null;
  return (
    <>
      <h3>Memory & chats</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        What BYTE remembers about you, and your saved chats. Everything is stored encrypted on this Mac only.
      </p>
      {error && <div className="banner danger">{error}</div>}
      {notice && <div className="banner">{notice}</div>}

      <div className="section">
        <label className="row" style={{ gap: 10, cursor: "pointer" }}>
          <input type="checkbox" checked={settings.memoryEnabled} onChange={(e) => void update({ memoryEnabled: e.target.checked })} />
          <span>
            <b>Use memory</b>
            <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
              BYTE uses what's below in every chat and suggests new things to remember (you confirm each one).
            </span>
          </span>
        </label>
      </div>

      <div className="section">
        <h4>About me</h4>
        <textarea
          className="about-me"
          rows={4}
          value={about}
          maxLength={1500}
          placeholder="For example: I'm a nurse in Denver. I like short answers with bullet points, and metric units."
          onChange={(e) => setAbout(e.target.value)}
          onBlur={() => about !== (settings.aboutMe ?? "") && void update({ aboutMe: about.trim() || null })}
        />
      </div>

      <div className="section">
        <h4>Saved memories ({memories.length})</h4>
        <div className="memory-list">
          {memories.length === 0 && <p className="faint" style={{ margin: 0 }}>Nothing yet. BYTE will suggest things as you chat, or add your own.</p>}
          {memories.map((m) => (
            <div key={m.id} className="memory-item">
              <input
                defaultValue={m.text}
                aria-label="Memory"
                onBlur={(e) => e.target.value !== m.text && void guard(async () => { await api.memoryUpdate(m.id, e.target.value); await refresh(); })}
              />
              <span className="faint" style={{ fontSize: "0.78em" }}>{m.source === "chat" ? "from a chat" : "added by you"}</span>
              <button className="icon-btn" title="Forget this" onClick={() => void guard(async () => { await api.memoryDelete(m.id); await refresh(); })}>
                <Trash2 size={14} />
              </button>
            </div>
          ))}
        </div>
        <form
          className="row"
          style={{ gap: 8 }}
          onSubmit={(e) => {
            e.preventDefault();
            if (!draft.trim()) return;
            void guard(async () => {
              await api.memoryAdd(draft);
              setDraft("");
              await refresh();
            });
          }}
        >
          <input className="text-input grow" value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="Add something BYTE should remember" aria-label="New memory" />
          <button className="btn sm" type="submit" disabled={!draft.trim()}><Plus size={14} /> Add</button>
        </form>
      </div>

      <div className="section">
        <h4>Your chats</h4>
        <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
          <button
            className="btn sm"
            onClick={() =>
              void guard(async () => {
                const dir = await openDialog({ directory: true, title: "Choose where to save the export" });
                if (typeof dir !== "string") return;
                const out = await api.chatsExport(dir);
                setNotice(`Exported to ${out} (a Markdown file per chat, plus byte-chats.json).`);
              })
            }
          >
            <Download size={14} /> Export all chats
          </button>
          <button
            className="btn sm ghost"
            style={{ color: "var(--danger)" }}
            onClick={() =>
              void guard(async () => {
                const ok = await ask("Erase every saved chat and memory? This can't be undone.", { title: "Erase everything", kind: "warning" });
                if (!ok) return;
                await api.dataWipe();
                await reloadChats();
                await refresh();
                setNotice("All chats and memories were erased.");
              })
            }
          >
            <Trash2 size={14} /> Erase all chats & memories
          </button>
        </div>
      </div>
    </>
  );
}

function AppearanceTab() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  if (!settings) return null;
  return (
    <>
      <h3>Appearance</h3>
      <p className="muted" style={{ marginTop: 0 }}>Pick a look. Changes apply instantly.</p>
      <div className="section">
        <h4>Theme</h4>
        <div className="theme-grid">
          {THEMES.map((t) => (
            <button key={t.id} className="theme-swatch" aria-pressed={settings.theme === t.id} onClick={() => void update({ theme: t.id })}>
              <span className="preview" style={{ background: t.id === "system" ? `linear-gradient(135deg, ${t.swatch[0]} 50%, ${t.swatch[1]} 50%)` : t.swatch[0] }}>
                <i style={{ background: t.swatch[1], border: "1px solid rgba(128,128,128,.3)" }} />
                <i style={{ background: t.swatch[2], boxShadow: `0 0 10px ${t.swatch[2]}` }} />
                <i style={{ background: t.swatch[3], boxShadow: `0 0 10px ${t.swatch[3]}` }} />
              </span>
              <span className="name">{t.name}</span>
            </button>
          ))}
        </div>
      </div>
      <div className="section">
        <h4>Layout</h4>
        <div className="field">
          <label>
            Text size
            <small>{Math.round(settings.fontScale * 100)}%</small>
          </label>
          <input
            type="range"
            min={0.85}
            max={1.3}
            step={0.05}
            value={settings.fontScale}
            onChange={(e) => void update({ fontScale: Number(e.target.value) })}
            aria-label="Text size"
          />
        </div>
        <div className="field">
          <label>Density</label>
          <div className="segmented">
            {(["comfortable", "compact"] as const).map((d) => (
              <button key={d} aria-pressed={settings.density === d} onClick={() => void update({ density: d })}>
                {d === "comfortable" ? "Comfortable" : "Compact"}
              </button>
            ))}
          </div>
        </div>
        <div className="field">
          <label>
            Show speed under answers
            <small>Tokens per second and time taken</small>
          </label>
          <input type="checkbox" checked={settings.showStats} onChange={(e) => void update({ showStats: e.target.checked })} />
        </div>
      </div>
    </>
  );
}

const CONTEXT_OPTIONS = [8192, 16384, 24576, 32768];

function EngineTab() {
  const engine = useStore((s) => s.engine);
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const models = useStore((s) => s.models);
  const [log, setLog] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadLog = async () => setLog(await api.engineLog());
  useEffect(() => {
    void loadLog();
  }, [engine.state]);

  const restart = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.engineRestart();
    } catch (e) {
      setError(errorText(e));
    }
    setBusy(false);
    void loadLog();
  };

  return (
    <>
      <h3>Engine</h3>
      <p className="muted" style={{ marginTop: 0 }}>BYTE's built-in AI engine (llama.cpp with Apple Metal) runs on this Mac.</p>
      {engine.state === "error" && <div className="banner danger">{engine.message}</div>}
      {error && <div className="banner danger">{error}</div>}
      <div className="field">
        <label>
          Status
          <small>
            {engine.state === "ready" && `Running ${displayName(models, engine.model, true)} with a ${contextLabel(engine.context)} context`}
            {engine.state === "starting" && "Loading the model…"}
            {engine.state === "noModel" && "No model downloaded yet"}
            {engine.state === "stopped" && "Stopped"}
            {engine.state === "error" && "Stopped because of an error"}
          </small>
        </label>
        <button className="btn sm" onClick={restart} disabled={busy || !settings?.activeModel}>
          <RefreshCw size={14} className={busy ? "spin" : undefined} /> Restart engine
        </button>
      </div>
      <div className="field">
        <label>
          Context window
          <small>How much of the conversation BYTE can see at once. Larger uses more memory; BYTE lowers it automatically if needed.</small>
        </label>
        <select
          value={settings?.contextSize ?? 16384}
          onChange={async (e) => {
            await update({ contextSize: Number(e.target.value) });
            void restart();
          }}
        >
          {CONTEXT_OPTIONS.map((c) => (
            <option key={c} value={c}>{contextLabel(c)} tokens{c === 16384 ? " (default)" : ""}</option>
          ))}
        </select>
      </div>
      <div className="section">
        <div className="row" style={{ justifyContent: "space-between" }}>
          <h4 style={{ margin: 0 }}>Engine log</h4>
          <button className="btn sm ghost" onClick={() => void loadLog()}>Refresh</button>
        </div>
        <pre className="log">{log.length ? log.slice(-200).join("\n") : "No output yet."}</pre>
      </div>
    </>
  );
}

function AboutTab() {
  const system = useStore((s) => s.system);
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [name, setName] = useState(settings?.userName ?? "");
  return (
    <>
      <div className="row" style={{ gap: 16 }}>
        <Logo size={56} />
        <div>
          <h3 style={{ margin: 0, letterSpacing: "0.14em" }}>BYTE</h3>
          <div className="muted">Version {__APP_VERSION__} · test build</div>
        </div>
      </div>
      <div className="section">
        <h4>You</h4>
        <div className="field">
          <label>
            Your name
            <small>BYTE uses it to greet you. Leave empty to skip.</small>
          </label>
          <input
            className="text-input"
            value={name}
            maxLength={40}
            placeholder="Your name"
            onChange={(e) => setName(e.target.value)}
            onBlur={() => void update({ userName: name.trim() || null })}
          />
        </div>
      </div>
      <div className="section">
        <h4>Privacy</h4>
        <p className="row" style={{ alignItems: "flex-start" }}>
          <ShieldCheck size={18} style={{ color: "var(--ok)", flex: "none", marginTop: 3 }} />
          <span>
            Your conversations and files never leave this Mac. The AI model runs locally on your GPU. BYTE only goes online to download models
            and to search and read the web when a question needs current information (turn this off with the Web button). Searches are
            sent to DuckDuckGo (or Bing as a fallback) without any account or identifying data.
          </span>
        </p>
      </div>
      {system && (
        <div className="section">
          <h4>This Mac</h4>
          <div className="field"><label>Chip</label><span className="muted">{system.chip}</span></div>
          <div className="field"><label>Memory</label><span className="muted">{ramSize(system.totalRamBytes)} · {bytes(system.gpuBudgetBytes)} usable by the GPU</span></div>
          <div className="field"><label>macOS</label><span className="muted">{system.osVersion}</span></div>
        </div>
      )}
      <div className="section">
        <h4>Help</h4>
        <button className="btn sm" onClick={() => void update({ onboardingComplete: false })}>Show the welcome guide again</button>
      </div>
    </>
  );
}
