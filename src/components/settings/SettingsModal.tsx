import { ask } from "@tauri-apps/plugin-dialog";
import { Cpu, HardDrive, Info, Palette, RefreshCw, ShieldCheck, X } from "lucide-react";
import { useEffect, useState } from "react";

import { THEMES } from "../../design/themes";
import { Logo } from "../../design/Logo";
import { api, errorText } from "../../lib/api";
import { bytes, contextLabel, ramSize } from "../../lib/format";
import { useStore, type SettingsTab } from "../../state/store";
import { ModelCard } from "../models/ModelCard";

const TABS: { id: SettingsTab; label: string; icon: typeof Cpu }[] = [
  { id: "models", label: "Models", icon: HardDrive },
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
          {tab === "appearance" && <AppearanceTab />}
          {tab === "engine" && <EngineTab />}
          {tab === "about" && <AboutTab />}
        </section>
      </div>
    </div>
  );
}

function ModelsTab() {
  const models = useStore((s) => s.models);
  const downloads = useStore((s) => s.downloads);
  const settings = useStore((s) => s.settings);
  const system = useStore((s) => s.system);
  const refresh = useStore((s) => s.refreshModels);
  const [error, setError] = useState<string | null>(null);

  const run = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    await refresh();
  };

  const chat = models.filter((m) => m.role === "chat");
  const helpers = models.filter((m) => m.role !== "chat");

  return (
    <>
      <h3>Models</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        Models run on this Mac's GPU. {system && <>You have <b>{ramSize(system.totalRamBytes)}</b> of memory and <b>{bytes(system.freeDiskBytes)}</b> free on disk.</>}
      </p>
      {error && <div className="banner danger">{error}</div>}
      <div className="section">
        <h4>Chat models</h4>
        <div className="model-list">
          {chat.map((m) => (
            <ModelCard
              key={m.id}
              model={m}
              dl={downloads[m.id]}
              active={settings?.activeModel === m.id && m.installed}
              onDownload={() => run(() => api.modelDownload(m.id))}
              onPause={() => run(() => api.modelPause(m.id))}
              onActivate={() => run(() => api.modelActivate(m.id))}
              onDelete={async () => {
                if (await ask(`Delete ${m.name}? You can download it again later.`, { title: "Delete model", kind: "warning" })) {
                  await run(() => api.modelDelete(m.id));
                }
              }}
            />
          ))}
        </div>
      </div>
      <div className="section">
        <h4>Helper models</h4>
        <p className="faint" style={{ marginTop: 0, fontSize: "0.9em" }}>
          Optional. Upcoming features (faster replies, knowledge base search) use these small models.
        </p>
        <div className="model-list">
          {helpers.map((m) => (
            <ModelCard
              key={m.id}
              model={m}
              dl={downloads[m.id]}
              onDownload={() => run(() => api.modelDownload(m.id))}
              onPause={() => run(() => api.modelPause(m.id))}
              onDelete={() => run(() => api.modelDelete(m.id))}
            />
          ))}
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
  const active = models.find((m) => m.id === settings?.activeModel);

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
            {engine.state === "ready" && `Running ${active?.name ?? engine.model} with a ${contextLabel(engine.context)} context`}
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
  const update = useStore((s) => s.updateSettings);
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
        <h4>Privacy</h4>
        <p className="row" style={{ alignItems: "flex-start" }}>
          <ShieldCheck size={18} style={{ color: "var(--ok)", flex: "none", marginTop: 3 }} />
          <span>
            Your conversations and files never leave this Mac. The AI model runs locally on your GPU. BYTE only goes online to download models
            and, in upcoming versions, to search the web when you ask a question that needs it.
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
