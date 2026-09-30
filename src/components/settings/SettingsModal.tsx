import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
import { Brain, Cloud, Cpu, Download, FolderSearch, Gauge, HardDrive, Info, Palette, Plus, RefreshCw, ShieldCheck, Trash2, UserRound, X, Zap } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { THEMES } from "../../design/themes";
import { Logo } from "../../design/Logo";
import { api, errorText } from "../../lib/api";
import { bytes, contextLabel, ramSize } from "../../lib/format";
import { displayName } from "../../lib/models";
import type { BoostInfo, GpuShare, LookerStatus, Memory, Profiles, Settings } from "../../lib/types";
import { useStore, type SettingsTab } from "../../state/store";
import { CatalogBrowser } from "../models/CatalogBrowser";
import { CloudTab } from "./CloudTab";
import { KnowledgeTab } from "./KnowledgeTab";
import { ModelLab } from "./ModelLab";
import { TuningPanel } from "./TuningPanel";

const TABS: { id: SettingsTab; label: string; icon: typeof Cpu }[] = [
  { id: "models", label: "Models", icon: HardDrive },
  { id: "memory", label: "Memory & chats", icon: Brain },
  { id: "knowledge", label: "Knowledge base", icon: FolderSearch },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "engine", label: "Engine", icon: Cpu },
  { id: "cloud", label: "Cloud", icon: Cloud },
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
          {tab === "knowledge" && <KnowledgeTab />}
          {tab === "appearance" && <AppearanceTab />}
          {tab === "engine" && <EngineTab />}
          {tab === "cloud" && <CloudTab />}
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
      <SpeedPrefPicker />
      <CatalogBrowser />
      <ModelLab />
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

const PREFS: { id: Settings["speedPref"]; label: string; hint: string }[] = [
  { id: "speed", label: "Faster", hint: "Quicker answers from a smaller or more compressed model" },
  { id: "balanced", label: "Balanced", hint: "Smart, at a comfortable reading speed" },
  { id: "quality", label: "Smarter", hint: "The most capable model that fits, even if slower" },
];

/** What "BYTE's pick" favours for this Mac. */
function SpeedPrefPicker() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const refresh = useStore((s) => s.refreshModels);
  if (!settings) return null;
  return (
    <div className="row pref-row">
      <span className="faint">BYTE's pick favours</span>
      <div className="segmented" role="group" aria-label="BYTE's pick favours">
        {PREFS.map((p) => (
          <button
            key={p.id}
            aria-pressed={settings.speedPref === p.id}
            title={p.hint}
            onClick={async () => {
              await update({ speedPref: p.id });
              await refresh();
            }}
          >
            {p.label}
          </button>
        ))}
      </div>
    </div>
  );
}

/** Speed boost (speculative decoding) and a real speed test on this Mac. */
export const RESEARCH_LEVELS = [
  { level: 0, label: "Normal", pages: "12 pages in Deep, 24 in Extended" },
  { level: 1, label: "More", pages: "20 pages in Deep, 32 in Extended" },
  { level: 2, label: "Max", pages: "32 pages in Deep, 48 in Extended" },
];

/** How much Deep and Extended research reads (Settings `researchDepth`). */
function ResearchSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  if (!settings) return null;
  const level = settings.researchDepth ?? 0;
  const current = RESEARCH_LEVELS[Math.min(2, level)];
  return (
    <div className="section">
      <h4>Research</h4>
      <div className="field">
        <label>
          How much to read
          <small>
            {current.pages}, plus fact-checks, comparisons and trips. More reads more sources and gives better answers, but takes longer.
          </small>
        </label>
        <div className="segmented" aria-label="Research depth">
          {RESEARCH_LEVELS.map((l) => (
            <button key={l.level} aria-pressed={level === l.level} onClick={() => void update({ researchDepth: l.level })}>
              {l.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function SpeedSection() {
  const engine = useStore((s) => s.engine);
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const downloads = useStore((s) => s.downloads);
  const [info, setInfo] = useState<BoostInfo | null>(null);
  const tune = useStore((s) => s.tune);
  const [error, setError] = useState<string | null>(null);
  const restarted = useRef(false);

  const load = () => api.speedBoostInfo().then(setInfo).catch(() => setInfo(null));
  useEffect(() => {
    void load();
  }, [engine.state, settings?.activeModel]);

  // When the helper finishes downloading, restart the engine to use it.
  const dl = info?.helperKey ? downloads[info.helperKey] : undefined;
  useEffect(() => {
    if (dl?.phase === "finished" && !restarted.current) {
      restarted.current = true;
      void api.engineRestart().then(load);
    }
  }, [dl?.phase]);

  if (!info || !settings) return null;
  const boosted = engine.state === "ready" && engine.boosted;
  const activeKey = engine.state === "ready" ? engine.model : (settings.activeModel ?? "");
  const tuned = settings.tuning?.[activeKey];
  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="section">
      <h4>Speed</h4>
      {error && <div className="banner danger">{error}</div>}
      <div className="field">
        <label>
          <span className="row" style={{ gap: 6 }}>
            <Zap size={14} style={{ color: "var(--accent)" }} /> Speed boost
          </span>
          <small>
            {!info.available
              ? "No helper for this model; after tuning, BYTE can still guess from text already in the chat."
              : boosted
                ? info.kind && info.kind !== "draft"
                  ? `On: ${info.helperName} guesses a few words ahead and your model checks them. Same answers, usually faster.`
                  : `On: ${info.helperName} drafts a few words ahead and your model checks them. Same answers, usually faster.`
                : settings.speedBoost && !info.installed
                  ? `Needs the ${info.helperName} helper (${bytes(info.helperBytes)} download).`
                  : settings.speedBoost
                    ? "On; it applies the next time the engine starts."
                    : "Off."}
          </small>
        </label>
        {info.available && (
          <div className="row" style={{ gap: 8 }}>
            {settings.speedBoost && !info.installed && info.helperKey && !dl && (
              <button className="btn sm primary" onClick={() => void guard(() => api.modelDownload(info.helperKey!))}>
                <Download size={14} /> Download helper
              </button>
            )}
            <label className="row" style={{ gap: 6, cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={settings.speedBoost}
                onChange={(e) =>
                  void guard(async () => {
                    await update({ speedBoost: e.target.checked });
                    await api.engineRestart();
                    await load();
                  })
                }
              />
              On
            </label>
          </div>
        )}
      </div>
      {dl && dl.phase !== "finished" && info.helperKey && (
        <div style={{ margin: "4px 0 10px" }}>
          <div className="progress">
            <span style={{ width: `${dl.total ? (dl.bytes / dl.total) * 100 : 0}%` }} />
          </div>
        </div>
      )}
      <div className="field">
        <label>
          <span className="row" style={{ gap: 6 }}>
            <Gauge size={14} style={{ color: "var(--accent)" }} /> Tuned for this Mac
          </span>
          <small>
            {tune
              ? `Tuning: step ${tune.step} of ${tune.total}, ${tune.label.toLowerCase()}…`
              : tuned
                ? `${tuned.tokensPerSec.toFixed(1)} tokens/sec writing · ${Math.round(tuned.promptPerSec)} tokens/sec reading · ` +
                  [
                    tuned.boost ? `Speed boost on (looks ${tuned.draftNMax} ahead)` : "Speed boost off",
                    ...(tuned.ngram ? ["repeated-text guessing"] : []),
                    tuned.kvF16 ? "full-precision memory" : "compact memory",
                    tuned.flashAttn ? "flash attention" : "no flash attention",
                    `batch ${tuned.ubatch}`,
                  ].join(", ") +
                  ` · ${tuned.thorough ? "thorough" : "quick"} tune, ${new Date(tuned.testedAt).toLocaleDateString()}`
                : "Not tuned yet. BYTE measures a few engine settings and keeps the fastest for this model (1–2 minutes)."}
          </small>
        </label>
        <div className="row" style={{ gap: 6, flex: "none" }}>
          <button
            className="btn sm"
            disabled={!!tune || engine.state !== "ready"}
            title="Tries Speed boost, repeated-text guessing, memory precision and batch size (1–2 minutes)"
            onClick={() =>
              void guard(async () => {
                await api.engineTune(false);
                await load();
              })
            }
          >
            <Gauge size={14} className={tune ? "spin" : undefined} /> {tune ? "Tuning…" : "Quick tune"}
          </button>
          <button
            className="btn sm primary"
            disabled={!!tune || engine.state !== "ready"}
            title="Also tries Speed boost look-ahead, flash attention and more batch sizes (about 5 minutes)"
            onClick={() =>
              void guard(async () => {
                await api.engineTune(true);
                await load();
              })
            }
          >
            Thorough tune
          </button>
        </div>
      </div>
      <div className="field">
        <label>
          Tune all downloaded models
          <small>Runs the thorough tune on every downloaded model that fits this Mac, one after another, then goes back to the one you use.</small>
        </label>
        <button
          className="btn sm"
          disabled={!!tune || engine.state !== "ready"}
          onClick={() =>
            void guard(async () => {
              await api.engineTuneAll(true);
              await load();
            })
          }
        >
          Tune all
        </button>
      </div>
      <GpuShareRow onError={setError} />
      <div className="field">
        <label>
          Tune new models automatically
          <small>The first time a model loads, BYTE spends a minute or two finding its fastest settings on this Mac.</small>
        </label>
        <input type="checkbox" checked={settings.autoTune} onChange={(e) => void update({ autoTune: e.target.checked })} aria-label="Tune new models automatically" />
      </div>
    </div>
  );
}

/** Optional: let the GPU use more memory so bigger models run fully on it. */
function GpuShareRow({ onError }: { onError: (e: string | null) => void }) {
  const [share, setShare] = useState<GpuShare | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.gpuShareInfo().then(setShare).catch(() => setShare(null));
  }, []);
  if (!share?.supported || (!share.raised && share.raisedBytes <= share.defaultBytes)) return null;
  const set = async (raise: boolean) => {
    onError(null);
    setBusy(true);
    try {
      setShare(await api.gpuShareSet(raise));
      await api.engineRestart().catch(() => undefined);
    } catch (e) {
      const msg = errorText(e);
      if (msg !== "Cancelled.") onError(msg);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="field">
      <label>
        Bigger GPU memory share
        <small>
          {share.raised
            ? `Raised: the GPU may use ${bytes(share.currentBytes)} (normally ${bytes(share.defaultBytes)}). Bigger models run fully on the GPU. Resets when the Mac restarts.`
            : `macOS lets the GPU use ${bytes(share.defaultBytes)} of memory. Raising it to ${bytes(share.raisedBytes)} lets bigger models run fully on the GPU (faster). Needs your Mac password; lasts until restart.`}
        </small>
      </label>
      <button className="btn sm" disabled={busy} onClick={() => void set(!share.raised)}>
        {share.raised ? "Reset" : "Raise"}
      </button>
    </div>
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
      <SpeedSection />
      <ResearchSection />
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
      <TuningPanel />
    </>
  );
}

function AboutTab() {
  const system = useStore((s) => s.system);
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [name, setName] = useState(settings?.userName ?? "");
  const [town, setTown] = useState(settings?.homePlace ?? "");
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
        <div className="field">
          <label htmlFor="home-place">
            Your town
            <small>For “near me” questions, like “coffee near me”. BYTE never looks up where you are by itself.</small>
          </label>
          <input
            id="home-place"
            className="text-input"
            value={town}
            maxLength={80}
            placeholder="e.g. Pittsburgh, PA"
            onChange={(e) => setTown(e.target.value)}
            onBlur={() => town.trim() !== (settings?.homePlace ?? "") && void update({ homePlace: town.trim() || null })}
          />
        </div>
      </div>
      <div className="section">
        <h4>Features</h4>
        <label className="field">
          <span>
            Kitchen
            <small>Recipes, “what can I make with…”, weekly meal plans and your recipe box (📖). Turn off to hide it.</small>
          </span>
          <input type="checkbox" checked={settings?.kitchenEnabled ?? true} onChange={(e) => void update({ kitchenEnabled: e.target.checked })} />
        </label>
        <div className="field">
          <span>
            Recipe measures
            <small>US: teaspoons, tablespoons, cups, ounces, pounds, °F. Metric: grams, millilitres, °C. Each recipe card can switch too.</small>
          </span>
          <span className="segmented" role="group" aria-label="Recipe measures">
            <button aria-pressed={(settings?.measureUnits ?? "us") === "us"} onClick={() => void update({ measureUnits: "us" })}>
              US
            </button>
            <button aria-pressed={settings?.measureUnits === "metric"} onClick={() => void update({ measureUnits: "metric" })}>
              Metric
            </button>
          </span>
        </div>
        <label className="field">
          <span>
            Web agent
            <small>
              BYTE can use a private browser for you: open sites, click, fill in forms, download files and save pages (“go to … and …”, or the Agent
              button). It always asks before submitting or downloading and never types passwords or card numbers.
            </small>
          </span>
          <input type="checkbox" checked={settings?.webAgentEnabled ?? true} onChange={(e) => void update({ webAgentEnabled: e.target.checked })} />
        </label>
        <PhotoHelperRow />
        <label className="field">
          <span>
            Writing studio
            <small>The ✍️ button (and a pen on every answer): rewrite, shorten, expand, change the tone or fix the grammar of any text.</small>
          </span>
          <input type="checkbox" checked={settings?.writingEnabled ?? true} onChange={(e) => void update({ writingEnabled: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Translate
            <small>“Translate this into Spanish”, “translate the attached file to German”, “translate &lt;link&gt; to English”: long texts are translated part by part.</small>
          </span>
          <input type="checkbox" checked={settings?.translateEnabled ?? true} onChange={(e) => void update({ translateEnabled: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Mac control
            <small>“Remind me to call Mom tomorrow at 3pm”, “make a note: …”, “what's on my calendar today?”, “turn on dark mode”, “play some jazz”, “run my Morning shortcut”, “check my email”, “reply to Sam’s email saying I can make it”, “text Mom that I’m running late”. BYTE asks before adding or changing anything, never sends email or texts itself (you press Send), and macOS asks once per app.</small>
          </span>
          <input type="checkbox" checked={settings?.macControl ?? true} onChange={(e) => void update({ macControl: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Job search
            <small>The 💼 tracker: paste a posting's link and BYTE fills it in; statuses, deadlines, notes and interview prep.</small>
          </span>
          <input type="checkbox" checked={settings?.jobsEnabled ?? true} onChange={(e) => void update({ jobsEnabled: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Assistants
            <small>The 🤖 button: BYTE set up for one job (an email helper, a study coach, your own) with its own instructions and starter prompts.</small>
          </span>
          <input type="checkbox" checked={settings?.assistantsEnabled ?? true} onChange={(e) => void update({ assistantsEnabled: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Study tools
            <small>Flashcards with spaced repetition (the 🎓 Study panel, Anki export), self-scoring quizzes, and the Tutor button.</small>
          </span>
          <input type="checkbox" checked={settings?.studyEnabled ?? true} onChange={(e) => void update({ studyEnabled: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Reviews
            <small>“Reviews of …”, “is … worth it”: ratings, pros and cons from review sites and owners.</small>
          </span>
          <input type="checkbox" checked={settings?.reviewsEnabled ?? true} onChange={(e) => void update({ reviewsEnabled: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Price compare
            <small>“Cheapest …”, “where to buy …”: prices read from the stores' own pages, cheapest first.</small>
          </span>
          <input type="checkbox" checked={settings?.pricesEnabled ?? true} onChange={(e) => void update({ pricesEnabled: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Game hints
            <small>Stuck in a game? Hints that get stronger one tap at a time, without spoilers.</small>
          </span>
          <input type="checkbox" checked={settings?.gameHintsEnabled ?? true} onChange={(e) => void update({ gameHintsEnabled: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Check answers against sources
            <small>Deep, Extended and fact-checks: BYTE checks each cited claim against its source and flags the ones it can't back.</small>
          </span>
          <input type="checkbox" checked={settings?.selfCheck ?? true} onChange={(e) => void update({ selfCheck: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Best of 3 for hard questions
            <small>Maths, logic and puzzles in Deep and Extended: three drafts, and the answer most of them reach. Slower, more often right.</small>
          </span>
          <input type="checkbox" checked={settings?.bestOfThree ?? true} onChange={(e) => void update({ bestOfThree: e.target.checked })} />
        </label>
      </div>
      <ProfilesSection />
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

/** Separate chats, memories and settings for different people (or work and
 * personal). Downloaded models are shared. Switching restarts BYTE. */
function ProfilesSection() {
  const [data, setData] = useState<Profiles | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const refresh = () => api.profilesList().then(setData).catch(() => setData(null));
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
  if (!data) return null;
  return (
    <div className="section">
      <h4>Profiles</h4>
      <p className="faint" style={{ marginTop: 0, fontSize: "0.88em" }}>
        Each profile has its own chats, memories and settings. Downloaded models are shared. Switching restarts BYTE.
      </p>
      {error && <div className="banner danger">{error}</div>}
      {data.profiles.map((p) => {
        const active = p.id === data.active;
        return (
          <div key={p.id} className={`profile-row ${active ? "active" : ""}`}>
            <UserRound size={15} style={{ color: active ? "var(--accent)" : undefined }} />
            <span className="grow">
              {p.name}
              {active && <span className="faint"> · in use</span>}
            </span>
            {!active && (
              <button
                className="btn sm"
                onClick={() =>
                  void guard(async () => {
                    if (await ask(`Switch to “${p.name}”? BYTE will restart.`, { title: "Switch profile" })) await api.profileSwitch(p.id);
                  })
                }
              >
                Switch
              </button>
            )}
            {!active && p.id !== "default" && (
              <button
                className="icon-btn"
                title="Delete profile"
                onClick={() =>
                  void guard(async () => {
                    const ok = await ask(`Delete the profile “${p.name}” and all of its chats and memories? This can't be undone.`, {
                      title: "Delete profile",
                      kind: "warning",
                    });
                    if (ok) {
                      await api.profileDelete(p.id);
                      await refresh();
                    }
                  })
                }
              >
                <Trash2 size={14} />
              </button>
            )}
          </div>
        );
      })}
      <form
        className="row"
        style={{ gap: 8 }}
        onSubmit={(e) => {
          e.preventDefault();
          void guard(async () => {
            await api.profileCreate(draft);
            setDraft("");
            await refresh();
          });
        }}
      >
        <input className="text-input grow" value={draft} maxLength={40} onChange={(e) => setDraft(e.target.value)} placeholder="New profile name" aria-label="New profile name" />
        <button className="btn sm" type="submit" disabled={!draft.trim()}>
          <Plus size={14} /> Add profile
        </button>
      </form>
    </div>
  );
}

/** Settings → Features: the photo helper (describes photos for models that can't see them). */
function PhotoHelperRow() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const downloads = useStore((s) => s.downloads);
  const [status, setStatus] = useState<LookerStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const on = settings?.photoHelper ?? true;
  const active = (status?.downloads ?? []).map((k) => downloads[k]).filter(Boolean);
  const busy = active.some((d) => ["downloading", "resuming", "verifying"].includes(d!.phase));
  const done = active.length > 0 && active.every((d) => d!.phase === "finished");
  useEffect(() => {
    api.lookerStatus().then(setStatus, (e) => setError(errorText(e)));
  }, [done]);
  const got = active.reduce((n, d) => n + (d!.bytes ?? 0), 0);
  const download = async () => {
    setError(null);
    try {
      for (const k of status?.downloads ?? []) await api.modelDownload(k);
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <div className="field">
      <span>
        Photo helper
        <small>
          When the model you use can't see images, a small model that can ({status?.model ? "downloaded" : `${bytes(status?.downloadBytes ?? 0)} download`}) looks at
          your photo and describes it, so you can still ask about it. It runs only while it's needed.
          {error && <span className="error"> {error}</span>}
        </small>
      </span>
      <span className="row" style={{ gap: 8 }}>
        {on && !status?.model && (status?.downloads.length ?? 0) > 0 && (
          <button className="btn sm" disabled={busy} onClick={() => void download()}>
            <Download size={14} />
            {busy && status ? `${Math.min(99, Math.round((got / Math.max(1, status.downloadBytes)) * 100))}%` : "Download"}
          </button>
        )}
        <input type="checkbox" checked={on} onChange={(e) => void update({ photoHelper: e.target.checked })} />
      </span>
    </div>
  );
}
