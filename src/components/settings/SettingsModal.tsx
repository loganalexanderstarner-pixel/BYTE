import { currentDevice, onDevice, ramLabel } from "../../lib/device";
import { VoiceControls } from "./VoiceControls"; // track:voice
import { fieldTitle, suggest } from "../../lib/settingsSuggest";
import { ByteVoicesRow, SpeakerLabelsRow, SpeechRows, VideoHelperRow, VoiceProfilesRow, WakeRow } from "./VoiceExtras";
import { VoiceModels } from "../chat/VoiceModels";
import { KeyboardSection } from "./KeyboardSection";
import { DiagnosticsButton } from "../DiagnosticsButton";
import { defaultSelectionKeys, isLinux, isPcOs, isWindows, prettyKeys } from "../../lib/keys";
import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
import { Download, Gauge, Plus, RefreshCw, ShieldCheck, Trash2, UserRound, X, Zap, Pencil } from "lucide-react";
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";

import { THEMES } from "../../design/themes";
import { Logo } from "../../design/Logo";
import { api, errorText, inTauri } from "../../lib/api";
import { BOOKMARKLET } from "../../lib/notes";
import { BALANCED, PRESETS, presetOf, SLIDERS, sliderWord } from "../../lib/personality";
import { ADULT_WARNING, needsAdultWarning, SWEAR_WARNING, TALK_MODES, type TalkMode } from "../../lib/talkMode";
import { topicList } from "../../lib/tasks";
import { bytes, contextLabel } from "../../lib/format";
import { displayName } from "../../lib/models";
import type { BoostInfo, GpuShare, LookerStatus, Memory, Profiles, Settings, Usage } from "../../lib/types";
import { bars } from "../../lib/dashboard";
import { onMac, useStore, type SettingsGroup, type SettingsTab } from "../../state/store";
import { visibleTabs } from "./tabs";
/* --- track:house begin --- */
// --- track:found begin ---
import { FoundBackupCards } from "../found/FoundSettings";
import { FoundBrowserToggles, FoundToggles, NeverTouchList } from "../found/FoundToggles";
// --- track:found end ---
import { BackupRestoreCard, FreeUpSpace, HouseToggles, ProblemReportCard } from "./HouseSettings";
/* --- track:house end --- */
import { CatalogBrowser } from "../models/CatalogBrowser";
import { ImageCatalog } from "../models/ImageCatalog";
import { CloudTab } from "./CloudTab";
import { PrivacyTab } from "./PrivacyTab";
import { BrowserSettings } from "./BrowserSettings"; // track:browser
import { UpdateRow } from "../update/UpdateRow";
import { customThemes, ThemeEditor } from "./ThemeEditor";
import type { CustomTheme } from "../../lib/customTheme";
import { ConnectorsTab } from "./ConnectorsTab";
/* --- track:connect begin --- */
import { ConnectSettings } from "./ConnectSettings";
/* --- track:connect end --- */
/* --- track:control begin --- */
import { ControlSettings } from "./ControlSettings";
import { PlanningSettings } from "./PlanningSettings"; // track:accuracy
/* --- track:control end --- */
import { KnowledgeTab } from "./KnowledgeTab";
import { ModelLab } from "./ModelLab";
import { TuningPanel } from "./TuningPanel";
import { engineWith, osText } from "../../lib/platform";
/* track:screen */ import { ScreenSettings } from "./ScreenSettings";
/* track:helpers */ import { HelperSettings } from "./HelperSettings";



/** Running on a PC: the Mac-only feature rows are reworded or left out. */
// Any computer that is not a Mac (Windows or Linux) gets PC words; Windows alone has PowerShell.
const pcHost = isPcOs();
const linuxHost = isLinux();

/** Which part of the long "About" page a group shows (every part shows in a search). */
const PartCtx = createContext<string>("all");

/** Shows its children only in the group `g` (or while searching, when every group is shown). */
function G({ g, children }: { g: string; children: ReactNode }) {
  const part = useContext(PartCtx);
  return part === "all" || part === g ? <>{children}</> : null;
}

/** What each old settings page id now lives under. */
export function groupOf(tab: SettingsTab): SettingsGroup {
  switch (tab) {
    case "appearance":
    case "about":
      return "general";
    case "engine":
      return "models";
    case "knowledge":
      return "memory";
    case "connectors":
      return "control";
    default:
      return tab;
  }
}

/** Does this text match every word of the search? (case-insensitive) */
export function matchesSearch(text: string, query: string): boolean {
  const t = text.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => t.includes(w));
}

function GroupBody({ group, about = true }: { group: SettingsGroup; about?: boolean }) {
  const settings = useStore((s) => s.settings);
  return (
    <PartCtx.Provider value={group}>
      {group === "general" && (
        <>
          <AppearanceTab />
          {about && <AboutTab />}
        </>
      )}
      {group === "models" && (
        <>
          <ModelsTab />
          <EngineTab />
          {about && <AboutTab />}
        </>
      )}
      {group === "images" && <ImagesTab />}
      {group === "voice" && (
        <>
          <VoiceSection />
          {about && <AboutTab />}
        </>
      )}
      {group === "control" && (
        <>
          {about && <AboutTab />}
          <ActionModeRow />
          <ControlSettings settings={settings} />
          <PlanningSettings settings={settings} /> {/* track:accuracy */}
          <ScreenSettings />
          <HelperSettings /> {/* track:helpers */}
          <BrowserSettings />
          <ConnectorsTab />
          {/* --- track:connect begin --- */}
          <ConnectSettings />
          {/* --- track:connect end --- */}
        </>
      )}
      {group === "privacy" && (
        <>
          <PrivacyTab />
          {about && <AboutTab />}
        </>
      )}
      {group === "memory" && (
        <>
          <MemoryTab />
          <KnowledgeTab />
          {about && <AboutTab />}
        </>
      )}
      {group === "cloud" && (
        <>
          <CloudTab />
          {about && <AboutTab />}
        </>
      )}
      {group === "backup" && about && <AboutTab />}
    </PartCtx.Provider>
  );
}
const isPhone = () => currentDevice() === "phone";

export function SettingsModal() {
  const tab = useStore((s) => s.settingsTab)!;
  const openSettings = useStore((s) => s.openSettings);
  const [query, setQuery] = useState("");
  const bodyRef = useRef<HTMLElement>(null);
  const group = groupOf(tab);
  const searching = query.trim().length > 0;
  // Type-ahead under the search box: the names of the settings on the page(s) now mounted, matched to what was typed.
  const [titles, setTitles] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [pick, setPick] = useState(0);
  const options = open ? suggest(titles, query) : [];
  const choose = (title: string) => {
    setQuery(title);
    setOpen(false);
    setPick(0);
    window.setTimeout(() => bodyRef.current?.querySelector<HTMLElement>(".field:not([hidden])")?.scrollIntoView({ block: "center", behavior: "smooth" }), 80);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && openSettings(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openSettings]);

  // The search hides the settings that do not match, without changing any of them. It runs again when the page fills in later.
  useEffect(() => {
    const body = bodyRef.current;
    if (!body) return;
    const collect = () => setTitles((old) => {
      const all = Array.from(body.querySelectorAll(".field")).map(fieldTitle).filter(Boolean);
      const next = Array.from(new Set([...old, ...all]));
      return next.length === old.length ? old : next;
    });
    const apply = () => {
      collect(); // (before the filter hides anything: the names of every mounted setting feed the type-ahead)
      filterSettings(body, query);
    };
    apply();
    if (!searching) return;
    const watcher = new MutationObserver(apply);
    watcher.observe(body, { childList: true, subtree: true });
    return () => watcher.disconnect();
  }, [query, searching, tab]);

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && openSettings(null)}>
      <div className="modal" role="dialog" aria-modal="true" aria-label="Settings">
        <nav className="modal-nav">
          <h2>Settings</h2>
          <div className="settings-search">
            <input
              className="text-input"
              type="search"
              value={query}
              placeholder="Search settings"
              aria-label="Search settings"
              role="combobox"
              aria-expanded={options.length > 0}
              aria-controls="settings-suggestions"
              autoComplete="off"
              onChange={(e) => {
                setQuery(e.target.value);
                setOpen(true);
                setPick(0);
              }}
              onFocus={() => setOpen(true)}
              onBlur={() => window.setTimeout(() => setOpen(false), 120)}
              onKeyDown={(e) => {
                if (!options.length) return;
                if (e.key === "ArrowDown") (e.preventDefault(), setPick((pick + 1) % options.length));
                else if (e.key === "ArrowUp") (e.preventDefault(), setPick((pick + options.length - 1) % options.length));
                else if (e.key === "Enter") (e.preventDefault(), choose(options[pick]));
                else if (e.key === "Escape") (e.stopPropagation(), setOpen(false));
              }}
            />
            {options.length > 0 && (
              <ul className="settings-suggest" id="settings-suggestions" role="listbox">
                {options.map((o, i) => (
                  <li key={o} role="option" aria-selected={i === pick} onMouseDown={(e) => (e.preventDefault(), choose(o))} onMouseEnter={() => setPick(i)}>
                    <b>{o.slice(0, query.trim().length)}</b>
                    {o.slice(query.trim().length)}
                  </li>
                ))}
              </ul>
            )}
          </div>
          {visibleTabs().map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              aria-current={!searching && group === id}
              onClick={() => {
                setQuery("");
                openSettings(id);
              }}
            >
              <Icon size={16} />
              {label}
            </button>
          ))}
        </nav>
        <section className="modal-body" ref={bodyRef}>
          <button className="icon-btn modal-close" onClick={() => openSettings(null)} aria-label="Close settings">
            <X size={18} />
          </button>
          {searching ? (
            <>
              {visibleTabs().map(({ id, label }) => (
                <div key={id} data-search-group={label}>
                  <h3 className="search-group">{label}</h3>
                  <GroupBody group={id} about={false} />
                </div>
              ))}
              {/* The long page of smaller settings is mounted once, all of it, while searching. */}
              <div data-search-group="More settings">
                <h3 className="search-group">More settings</h3>
                <PartCtx.Provider value="all">
                  <AboutTab />
                </PartCtx.Provider>
              </div>
            </>
          ) : (
            <GroupBody group={group} />
          )}
        </section>
      </div>
    </div>
  );
}

/** Hides what does not match `query` inside `root` (nothing hidden for an empty query). Only the `hidden` attribute is touched. */
function filterSettings(root: HTMLElement, query: string) {
  const q = query.trim();
  const hide = (el: Element, yes: boolean) => {
    if (yes) el.setAttribute("hidden", "");
    else el.removeAttribute("hidden");
  };
  const fields = root.querySelectorAll<HTMLElement>(".field");
  fields.forEach((f) => hide(f, !!q && !matchesSearch(f.textContent ?? "", q)));
  root.querySelectorAll<HTMLElement>(".section").forEach((sec) => {
    const own = sec.querySelectorAll(".field");
    const heading = sec.querySelector("h3, h4")?.textContent ?? "";
    // A section with nothing in it (every part of it belongs to another group) shows nothing.
    if (!(sec.textContent ?? "").trim() && !sec.querySelector("input, button, select, textarea")) return hide(sec, true);
    if (!q || own.length === 0) return hide(sec, false);
    if (matchesSearch(heading, q)) {
      own.forEach((f) => hide(f, false));
      return hide(sec, false);
    }
    hide(sec, ![...own].some((f) => !f.hasAttribute("hidden")));
  });
  root.querySelectorAll<HTMLElement>("[data-search-group]").forEach((g) => {
    const label = g.getAttribute("data-search-group") ?? "";
    const anything = [...g.querySelectorAll(".field")].some((f) => !f.hasAttribute("hidden"));
    hide(g, !!q && !anything && !matchesSearch(label, q));
  });
}

/** Everything about making and editing pictures (Windows). */
function ImagesTab() {
  return (
    <>
      <h3>Images</h3>
      <ImageCatalog />
    </>
  );
}

function ModelsTab() {
  return (
    <>
      <h3>Models</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        {onDevice(currentDevice() === "phone" ? "Pick the brain BYTE runs on. Everything runs on this phone's own processor; files download from Hugging Face only when you choose them." : "Pick the brain BYTE runs on. Everything runs on this Mac's GPU; files download from Hugging Face only when you choose them.")}
      </p>
      <SpeedPrefPicker />
      <CatalogBrowser />
      <ModelLab />
    </>
  );
}

const VOICE_LANGUAGES: [string, string][] = [
  ["auto", "Detect it"],
  ["en", "English"],
  ["es", "Spanish"],
  ["fr", "French"],
  ["de", "German"],
  ["it", "Italian"],
  ["pt", "Portuguese"],
  ["nl", "Dutch"],
  ["pl", "Polish"],
  ["ru", "Russian"],
  ["uk", "Ukrainian"],
  ["zh", "Chinese"],
  ["ja", "Japanese"],
  ["ko", "Korean"],
  ["hi", "Hindi"],
  ["ar", "Arabic"],
  ["tr", "Turkish"],
];

/** Voice input (voice.rs): speech models, language, on/off. */
function VoiceSection() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const on = settings?.voiceEnabled !== false;
  return (
    <div className="section">
      <h4>Voice</h4>
      <label className="field">
        <span>
          Voice input
          <small>{osText("Click 🎤 in the message box, or hold Space in an empty box, and talk; what you say is typed out for you to check and send. Audio files you attach (WAV, MP3, M4A…) become transcripts, with who said what when speaker labels are on. It all happens on this Mac.")}</small>
        </span>
        <input type="checkbox" checked={on} onChange={(e) => void update({ voiceEnabled: e.target.checked })} />
      </label>
      {on && (
        <>
          <VoiceModels />
          <label className="field">
            <span>
              Spoken language
              <small>“Detect it” works for most; picking yours helps with short phrases. The English model always hears English.</small>
            </span>
            <select value={settings?.voiceLanguage ?? "auto"} onChange={(e) => void update({ voiceLanguage: e.target.value })} aria-label="Spoken language">
              {VOICE_LANGUAGES.map(([code, name]) => (
                <option key={code} value={code}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          {/* On a phone: voice input only. Reading aloud, "Hey BYTE", speaker labels and the video helper need engines that aren't built for Android yet. */}
          {!isPhone() && (
            <>
              <SpeechRows />
              <ByteVoicesRow />
              <WakeRow />
              <VoiceControls /> {/* track:voice */}
              <VoiceProfilesRow />
              <SpeakerLabelsRow />
              <VideoHelperRow />
            </>
          )}
        </>
      )}
    </div>
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
        What BYTE remembers about you, and your saved chats. {onDevice("Everything is stored encrypted on this Mac only.")}
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

  // A Mac has one pool of memory; on a PC the model can sit on the card, in system memory, or both.
  const pcWithCard = !onMac();
  return (
    <div className="section">
      <h4>Speed</h4>
      {error && <div className="banner danger">{error}</div>}
      {pcWithCard && (
        <div className="field">
          <label>
            <span>Where the model lives</span>
            <small>
              {settings.memoryMode === "gpu"
                ? "Graphics memory only: the fastest, but only models that fit on your card."
                : settings.memoryMode === "gpuRam"
                  ? "Graphics memory and system memory together: bigger models fit, and the part in system memory runs slower."
                  : settings.memoryMode === "cpu"
                    ? "Processor and system memory only: slower, but your graphics card stays free for games."
                    : "Automatic: the card first, spilling to memory only when it helps."}
            </small>
          </label>
          <select
            value={settings.memoryMode ?? "auto"}
            aria-label="Where the model lives"
            onChange={(e) =>
              void guard(async () => {
                await update({ memoryMode: e.target.value as "auto" | "gpu" | "gpuRam" | "cpu" });
                await api.engineRestart();
                await load();
              })
            }
          >
            <option value="auto">Automatic</option>
            <option value="gpu">Graphics memory only</option>
            <option value="gpuRam">Graphics + system memory</option>
            <option value="cpu">Processor and system memory only</option>
          </select>
        </div>
      )}
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
            <Gauge size={14} style={{ color: "var(--accent)" }} /> {onDevice("Tuned for this Mac")}
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
          <small>{onDevice("Runs the thorough tune on every downloaded model that fits this Mac, one after another, then goes back to the one you use.")}</small>
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
          <small>{onDevice("The first time a model loads, BYTE spends a minute or two finding its fastest settings on this Mac.")}</small>
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
  if (isPhone() || !share?.supported || (!share.raised && share.raisedBytes <= share.defaultBytes)) return null;
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
            ? osText(`Raised: the GPU may use ${bytes(share.currentBytes)} (normally ${bytes(share.defaultBytes)}). Bigger models run fully on the GPU. Resets when the Mac restarts.`)
            : osText(`macOS lets the GPU use ${bytes(share.defaultBytes)} of memory. Raising it to ${bytes(share.raisedBytes)} lets bigger models run fully on the GPU (faster). Needs your Mac password; lasts until restart.`)}
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
        <CustomThemes />
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
            Reduce motion
            <small>{osText("Fewer animations. Auto follows macOS (System Settings → Accessibility → Display).")}</small>
          </label>
          <div className="segmented">
            {(["auto", "reduce"] as const).map((m) => (
              <button key={m} aria-pressed={(settings.reduceMotion ?? "auto") === m} onClick={() => void update({ reduceMotion: m })}>
                {m === "auto" ? "Auto" : "Reduce"}
              </button>
            ))}
          </div>
        </div>
        <div className="field">
          <label>
            Sounds
            <small>A soft chime when an answer is ready</small>
          </label>
          <input type="checkbox" checked={!!settings.sounds} onChange={(e) => void update({ sounds: e.target.checked })} aria-label="Sounds" />
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

/** Your own themes, plus "Make your own" (ThemeEditor). */
function CustomThemes() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [editing, setEditing] = useState<CustomTheme | null | "new">(null);
  const list = customThemes(settings?.customThemes);
  return (
    <div className="custom-themes">
      {editing ? (
        <ThemeEditor editing={editing === "new" ? null : editing} onClose={() => setEditing(null)} />
      ) : (
        <div className="row" style={{ gap: 6, flexWrap: "wrap", marginBottom: 8 }}>
          {list.map((t) => (
            <span key={t.id} className="custom-theme-chip">
              <button className="chip" aria-pressed={settings?.theme === t.id} onClick={() => void update({ theme: t.id })}>
                <i style={{ background: t.colors.accent }} /> {t.name}
              </button>
              <button className="icon-btn sm" onClick={() => setEditing(t)} aria-label={`Edit ${t.name}`}>
                <Pencil size={12} />
              </button>
            </span>
          ))}
          <button className="btn sm ghost" onClick={() => setEditing("new")}>
            <Plus size={13} /> Make your own
          </button>
        </div>
      )}
    </div>
  );
}

const CONTEXT_OPTIONS = [8192, 16384, 24576, 32768];

function EngineTab() {
  const engine = useStore((s) => s.engine);
  const system = useStore((s) => s.system);
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
      <p className="muted" style={{ marginTop: 0 }}>{currentDevice() === "phone" ? "BYTE's built-in AI engine (llama.cpp) runs on this phone's processor." : osText(`BYTE's built-in AI engine (${engineWith(system)}) runs on this Mac.`)}</p>
      {engine.state === "error" && (
        <div className="banner danger">
          <span className="grow">{engine.message}</span>
          <DiagnosticsButton />
        </div>
      )}
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

/** Local usage stats (Rust `dashboard::usage`): counted from chats on this Mac, never sent anywhere. */
function UsageSection() {
  const [u, setU] = useState<Usage | null>(null);
  useEffect(() => {
    api.dashboardUsage().then(setU, () => setU(null));
  }, []);
  if (!u || u.chats === 0) return null;
  const heights = bars(u.perDay);
  return (
    <div className="section">
      <h4>Your usage</h4>
      <p className="muted small" style={{ marginTop: 0 }}>
        {onDevice("Counted from the chats on this Mac; nothing is sent anywhere.")}
      </p>
      <div className="usage-grid">
        <div>
          <b>{u.questionsWeek}</b>
          <span className="muted small">questions this week</span>
        </div>
        <div>
          <b>{u.chats}</b>
          <span className="muted small">chats ({u.chatsWeek} this week)</span>
        </div>
        <div>
          <b>{u.citedAnswers}</b>
          <span className="muted small">answers with sources</span>
        </div>
        {u.avgSpeed != null && (
          <div>
            <b>{u.avgSpeed}</b>
            <span className="muted small">tokens per second on average (30 days)</span>
          </div>
        )}
      </div>
      <div className="usage-bars" role="img" aria-label={`Questions per day, last 14 days: ${u.perDay.join(", ")}`}>
        {heights.map((h, i) => (
          <span key={i} style={{ height: `${Math.max(4, Math.round(h * 100))}%` }} title={`${u.perDay[i]} questions`} />
        ))}
      </div>
      <p className="muted small">Questions per day, last 14 days{u.modes.length > 0 && ` · most used: ${u.modes.map(([m, n]) => `${m} (${n})`).join(", ")}`}</p>
    </div>
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
      <G g="general">
      <div className="row" style={{ gap: 16 }}>
        <Logo size={56} />
        <div>
          <h3 style={{ margin: 0, letterSpacing: "0.14em" }}>BYTE</h3>
          <div className="muted">Version {__APP_VERSION__}</div>
        </div>
      </div>
      <UpdateRow />
      </G>
      <G g="general">
      <div className="section">
        <h4>Something wrong?</h4>
        <div className="field">
          <label>
            Diagnostics
            <small>Copies your device, memory, engine status and recent engine log. Nothing from your chats, no keys.</small>
          </label>
          <DiagnosticsButton />
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
      <PersonalitySection />
      </G>
      <G g="models">
        <UsageSection />
      </G>
      <G g="general">
        {!isPhone() && <KeyboardSection settings={settings} />}
      </G>
      {/* --- track:house begin --- */}
      <G g="models">
        <HouseToggles group="models" />
        <FreeUpSpace />
      </G>
      <G g="voice">
        <HouseToggles group="voice" />
      </G>
      <G g="memory">
        <HouseToggles group="memory" />
      </G>
      <G g="backup">
        <BackupRestoreCard />
        <ProblemReportCard />
      </G>
      {/* --- track:house end --- */}
      {/* --- track:found begin --- */}
      <G g="backup">
        <FoundBackupCards />
      </G>
      <G g="general">
        <FoundToggles />
      </G>
      <G g="control">
        <NeverTouchList />
        <FoundBrowserToggles />
      </G>
      {/* --- track:found end --- */}
      <G g="general">
      <div className="section">
        <h4>In the background</h4>
        <label className="field">
          <span>
            Open BYTE at login
            <small>BYTE starts quietly (no window) when you log in, so schedules, watched pages and automations run all day. {pcHost ? "Open it from the system tray icon." : "Click BYTE in the Dock to open it."}</small>
          </span>
          <input type="checkbox" checked={settings?.openAtLogin ?? false} onChange={(e) => void update({ openAtLogin: e.target.checked })} />
        </label>
        <label className="field">
          <span>
            Keep running when the window is closed
            <small>Closing the window hides BYTE instead of quitting, so it keeps working. {pcHost ? "Quit from the system tray icon's menu." : "Quit with ⌘Q."}</small>
          </span>
          <input type="checkbox" checked={settings?.keepRunning ?? true} onChange={(e) => void update({ keepRunning: e.target.checked })} />
        </label>
      </div>
      </G>
      <div className="section">
        <G g="general"><h4>Features</h4></G>
        <G g="control"><h4>Control and tools</h4></G>
        <G g="memory"><h4>To-dos, notes and the daily briefing</h4></G>
        <G g="models"><h4>Checking answers</h4></G>
        <G g="general">
        <label className="field">
          <span>
            Kitchen
            <small>Recipes, “what can I make with…”, weekly meal plans and your recipe box (📖). Turn off to hide it.</small>
          </span>
          <input type="checkbox" checked={settings?.kitchenEnabled ?? true} onChange={(e) => void update({ kitchenEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="general">
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
        </G>
        <G g="control">
{!isPhone() && (
<>
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
</>
)}
        </G>
        <G g="general">
        <PhotoHelperRow />
        </G>
        <G g="general">
        <label className="field">
          <span>
            Writing studio
            <small>The ✍️ button (and a pen on every answer): rewrite, shorten, expand, change the tone or fix the grammar of any text.</small>
          </span>
          <input type="checkbox" checked={settings?.writingEnabled ?? true} onChange={(e) => void update({ writingEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="memory">
        <NotesRow />
        </G>
        <G g="general">
        <label className="field">
          <span>
            Translate
            <small>“Translate this into Spanish”, “translate the attached file to German”, “translate &lt;link&gt; to English”: long texts are translated part by part.</small>
          </span>
          <input type="checkbox" checked={settings?.translateEnabled ?? true} onChange={(e) => void update({ translateEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="control">
{!isPhone() && (
<>
        <label className="field">
          <span>
            {linuxHost ? "Hotkeys and clipboard" : pcHost ? "PC control, hotkeys and clipboard" : osText("Mac control")}
            <small>{linuxHost ? "Lets BYTE read the text you select in other apps (the hotkey below) and keep a clipboard history. Controlling other apps and system settings by voice or text isn't available on Linux yet." : pcHost ? "Lets BYTE do things on this PC when you ask (dark mode, volume, Wi-Fi, music, Windows settings, email and calendar drafts), find files (“find my tax return pdf”), tidy a folder (“organize my Downloads”), shrink or convert the photos you select in File Explorer, read the text you select in other apps (the hotkey below) and keep a clipboard history. Reminders and notes stay inside BYTE." : osText("“Remind me to call Mom tomorrow at 3pm”, “make a note: …”, “what's on my calendar today?”, “turn on dark mode”, “play some jazz”, “run my Morning shortcut”, “check my email”, “reply to Sam’s email saying I can make it”, “text Mom that I’m running late”, “find my tax return pdf”, “organize my Downloads”, “convert the selected photos to jpg”, “run a command to show my disk space”. BYTE asks before adding or changing anything, never sends email or texts itself (you press Send), and macOS asks once per app.")}</small>
          </span>
          <input type="checkbox" checked={settings?.macControl ?? true} onChange={(e) => void update({ macControl: e.target.checked })} />
        </label>
</>
)}
        </G>
        <G g="control">
{!isPhone() && (
<>
        {isWindows() && (
          <label className="field">
            <span>
              PowerShell commands
              <small>For questions about your PC (“run a command to show my disk space”, “what's using port 3000?”), BYTE proposes one PowerShell command, explains it in plain words and runs it only after you press Do it. It only runs commands that read or show information, plus a few careful ones that move or delete a single file. Formatting, the registry, downloads, other programs and anything that needs administrator rights are never run. Off until you turn it on.</small>
            </span>
            <input type="checkbox" checked={settings?.terminalEnabled ?? false} onChange={(e) => void update({ terminalEnabled: e.target.checked })} />
          </label>
        )}
</>
)}
        </G>
        <G g="memory">
        <label className="field">
          <span>
            To-do list and schedules
            <small>The ✅ panel: to-dos with reminders (“add pay rent by Friday to my to-do list”), a daily briefing, and questions BYTE asks on a schedule (“every weekday at 8am, summarize the latest AI news”). They run while BYTE is open.</small>
          </span>
          <input type="checkbox" checked={settings?.tasksEnabled ?? true} onChange={(e) => void update({ tasksEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="memory">
        <label className="field">
          <span>
            Daily briefing topics
            <small>News topics the briefing follows, separated by commas (up to 5; needs the web). Say “brief me” any time.</small>
          </span>
          <input
            type="text"
            className="topics-input"
            disabled={settings?.tasksEnabled === false}
            defaultValue={(settings?.briefingTopics ?? []).join(", ")}
            placeholder="AI, Steelers, climate"
            onBlur={(e) => void update({ briefingTopics: topicList(e.target.value) })}
            aria-label="Daily briefing topics"
          />
        </label>
        </G>
        <G g="general">
        <label className="field">
          <span>
            News feeds and watched pages
            <small>Follow sites (“follow theverge.com”, then “what's new in my feeds?”) and watch pages for changes or price drops (“tell me when &lt;link&gt; drops below $200”). BYTE checks while it's open and sends a notification. Needs web access.</small>
          </span>
          <input type="checkbox" checked={settings?.watchEnabled ?? true} onChange={(e) => void update({ watchEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="control">
        <label className="field">
          <span>
            Automations
            <small>{osText("Steps BYTE does one after another (“every weekday at 8am, find AI news, then summarize it, then save it to a file”), when you run them, on a schedule or when BYTE opens. Build them in the ✅ panel; add one to Shortcuts to run it from the menu bar or with Siri.")}</small>
          </span>
          <input type="checkbox" checked={settings?.automationsEnabled ?? true} onChange={(e) => void update({ automationsEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="general">
        <label className="field">
          <span>
            Trackers
            <small>{osText("Packages (“track 1Z…”), bills and subscriptions (“add Netflix $15.49 a month on the 12th”, “what subscriptions do I have?”), birthdays with gift ideas (“Sam's birthday is March 3”) and car and home maintenance (“change the furnace filter every 3 months”), with notifications ahead of time. All kept on this Mac.")}</small>
          </span>
          <input type="checkbox" checked={settings?.trackersEnabled ?? true} onChange={(e) => void update({ trackersEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="control">
{!isPhone() && (
<>
        {(!pcHost || isWindows()) && (
        <label className="field">
          <span>
            {isWindows() ? "PC upkeep" : osText("Mac upkeep")}
            <small>{isWindows() ? "“What's taking up space?”, “find duplicate files”, “why is my PC slow?”, “check my PC”, “what opens at startup?”, “uninstall Zoom”. BYTE looks first and asks before it changes anything. Files you choose go to the Recycle Bin (Undo puts them back), a startup app is only switched off (Undo switches it on), and uninstalling runs the program's own uninstaller, which can't be undone. BYTE leaves Windows itself, drivers and your antivirus alone." : osText("“What's taking up space?”, “find duplicate files”, “why is my Mac slow?”, “what's draining my battery?”, “check my Mac”, “uninstall Zoom”, “what opens at login?”. Anything removed goes to the Trash (Undo puts it back); BYTE never empties the Trash or touches macOS itself.")}</small>
          </span>
          <input type="checkbox" disabled={settings?.macControl === false} checked={settings?.macUpkeep ?? true} onChange={(e) => void update({ macUpkeep: e.target.checked })} />
        </label>
        )}
</>
)}
        </G>
        <G g="control">
{!isPhone() && (
<>
        <label className="field">
          <span>
            Selected text hotkey ({prettyKeys(settings?.selectionKeys ?? defaultSelectionKeys())})
            <small>{osText("Select text in any app and press the hotkey (Keyboard and menu bar, in About, changes it): it opens in the writing studio to rewrite, fix, translate, reply to or explain, then “Paste into” puts the result back.") + (pcHost ? " It can't read an app that is running as administrator." : osText(" macOS asks once to allow BYTE in Accessibility."))}</small>
          </span>
          <input type="checkbox" disabled={settings?.macControl === false} checked={settings?.selectionHotkey ?? true} onChange={(e) => void update({ selectionHotkey: e.target.checked })} />
        </label>
</>
)}
        </G>
        <G g="control">
{!isPhone() && (
<>
        <label className="field">
          <span>
            Clipboard history
            <small>{osText("Keeps the last 200 things you copy, searchable (📋 in the top bar). Off until you switch it on. Passwords from password managers and text that looks like a password or key are never kept; it's stored encrypted on this Mac.")}</small>
          </span>
          <input type="checkbox" disabled={settings?.macControl === false} checked={settings?.clipboardHistory ?? false} onChange={(e) => void update({ clipboardHistory: e.target.checked })} />
        </label>
</>
)}
        </G>
        <G g="general">
        <label className="field">
          <span>
            Job search
            <small>The 💼 tracker: paste a posting's link and BYTE fills it in; statuses, deadlines, notes and interview prep.</small>
          </span>
          <input type="checkbox" checked={settings?.jobsEnabled ?? true} onChange={(e) => void update({ jobsEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="general">
        <label className="field">
          <span>
            Assistants
            <small>The 🤖 button: BYTE set up for one job (an email helper, a study coach, your own) with its own instructions and starter prompts.</small>
          </span>
          <input type="checkbox" checked={settings?.assistantsEnabled ?? true} onChange={(e) => void update({ assistantsEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="general">
        <label className="field">
          <span>
            Study tools
            <small>Flashcards with spaced repetition (the 🎓 Study panel, Anki export), self-scoring quizzes, and the Tutor button.</small>
          </span>
          <input type="checkbox" checked={settings?.studyEnabled ?? true} onChange={(e) => void update({ studyEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="general">
        <label className="field">
          <span>
            Reviews
            <small>“Reviews of …”, “is … worth it”: ratings, pros and cons from review sites and owners.</small>
          </span>
          <input type="checkbox" checked={settings?.reviewsEnabled ?? true} onChange={(e) => void update({ reviewsEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="general">
        <label className="field">
          <span>
            Price compare
            <small>“Cheapest …”, “where to buy …”: prices read from the stores' own pages, cheapest first.</small>
          </span>
          <input type="checkbox" checked={settings?.pricesEnabled ?? true} onChange={(e) => void update({ pricesEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="general">
        <label className="field">
          <span>
            Game hints
            <small>Stuck in a game? Hints that get stronger one tap at a time, without spoilers.</small>
          </span>
          <input type="checkbox" checked={settings?.gameHintsEnabled ?? true} onChange={(e) => void update({ gameHintsEnabled: e.target.checked })} />
        </label>
        </G>
        <G g="models">
        <label className="field">
          <span>
            Check answers against sources
            <small>Deep, Extended and fact-checks: BYTE checks each cited claim against its source and flags the ones it can't back.</small>
          </span>
          <input type="checkbox" checked={settings?.selfCheck ?? true} onChange={(e) => void update({ selfCheck: e.target.checked })} />
        </label>
        </G>
        <G g="models">
        <label className="field">
          <span>
            Best of 3 for hard questions
            <small>Maths, logic and puzzles in Deep and Extended: three drafts, and the answer most of them reach. Slower, more often right.</small>
          </span>
          <input type="checkbox" checked={settings?.bestOfThree ?? true} onChange={(e) => void update({ bestOfThree: e.target.checked })} />
        </label>
        </G>
      </div>
      <G g="cloud">
      <ProfilesSection />
      </G>
      <G g="privacy">
      <div className="section">
        <h4>Privacy</h4>
        <p className="row" style={{ alignItems: "flex-start" }}>
          <ShieldCheck size={18} style={{ color: "var(--ok)", flex: "none", marginTop: 3 }} />
          <span>
            {osText("Your conversations and files never leave this Mac. The AI model runs locally on your GPU. BYTE only goes online to download models and to search and read the web when a question needs current information (turn this off with the Web button). Searches are sent to DuckDuckGo (or Bing as a fallback) without any account or identifying data.")}
          </span>
        </p>
      </div>
      {system && (
        <div className="section">
          <h4>{onDevice("This Mac")}</h4>
          <div className="field"><label>Chip</label><span className="muted">{system.chip}</span></div>
          <div className="field"><label>Memory</label><span className="muted">{ramLabel(system)} · {bytes(system.gpuBudgetBytes)} {system.phone ? "BYTE can use for models" : "usable by the GPU"}</span></div>
          <div className="field"><label>{isPhone() ? "Android" : osText("macOS")}</label><span className="muted">{system.osVersion}</span></div>
        </div>
      )}
      </G>
      <G g="backup">
      <div className="section">
        <h4>Help</h4>
        <button className="btn sm" onClick={() => void update({ onboardingComplete: false })}>Show the welcome guide again</button>
      </div>
      </G>
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

/** Settings → Features: notes, where they're kept, and the web clipper bookmarklet. */
function NotesRow() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [dir, setDir] = useState<string | null>(null);
  useEffect(() => {
    if (inTauri && settings?.notesEnabled !== false) void api.notesInfo().then((i) => setDir(i.dir), () => undefined);
  }, [settings?.notesEnabled, settings?.notesDir]);
  const on = settings?.notesEnabled !== false;
  return (
    <div className="field" style={{ display: "block" }}>
      <label className="row" style={{ alignItems: "flex-start", gap: 12 }}>
        <span className="grow">
          Notes and web clipper
          <small>
            The 📝 button: Markdown notes with folders and tags, saved as plain files you own{dir ? ` (${dir.replace(/^\/Users\/[^/]+/, "~")})` : ""}. Save any answer with 📝 under it, see any answer
            as a mind map, and clip web pages. With the knowledge base on, BYTE can answer from your notes.
          </small>
        </span>
        <input type="checkbox" checked={on} onChange={(e) => void update({ notesEnabled: e.target.checked })} />
      </label>
      {on && (
        <div className="row" style={{ gap: 8, marginTop: 8, flexWrap: "wrap" }}>
          <button
            className="btn sm ghost"
            onClick={async () => {
              const picked = await openDialog({ directory: true, title: "Where to keep your notes" }).catch(() => null);
              if (typeof picked === "string") await update({ notesDir: picked });
            }}
          >
            Change folder…
          </button>
          {settings?.notesDir && (
            <button className="btn sm ghost" onClick={() => void update({ notesDir: null })}>
              Use Documents/BYTE/Notes
            </button>
          )}
          <span className="faint small">Web clipper: drag this to your browser's bookmarks bar, then click it on any page →</span>
          <a className="btn sm primary" href={BOOKMARKLET} onClick={(e) => e.preventDefault()} draggable title="Drag me to the bookmarks bar">
            Clip to BYTE
          </a>
        </div>
      )}
    </div>
  );
}

/** Settings → About → Personality: the style modes (Casual, Direct, Unhinged, Auto; style.rs). */
function TalkModes() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const mode = (settings?.talkMode ?? "") as TalkMode;
  const kids = !!settings?.kidsMode;
  const [learned, setLearned] = useState("");
  useEffect(() => {
    if (inTauri && mode === "auto") void api.styleSummary().then(setLearned, () => setLearned(""));
  }, [mode]);
  if (kids) return null;
  const pick = (next: TalkMode) => {
    if (needsAdultWarning(next, settings?.allowSwearing) && !window.confirm(ADULT_WARNING)) return;
    void update(next === "unhinged" ? { talkMode: next, allowSwearing: true } : { talkMode: next });
  };
  return (
    <div style={{ marginBottom: 12 }}>
      <div className="chips" role="group" aria-label="How BYTE talks">
        {TALK_MODES.map((m) => (
          <button key={m.id || "sliders"} className="chip" aria-pressed={mode === m.id} title={m.hint} onClick={() => pick(m.id)}>
            {m.name}
          </button>
        ))}
      </div>
      <p className="faint small" style={{ margin: "6px 0 0" }}>{TALK_MODES.find((m) => m.id === mode)?.hint}</p>
      {mode === "auto" && (
        <div style={{ marginTop: 6 }}>
          <label className="check small">
            <input
              type="checkbox"
              checked={!!settings?.allowSwearing}
              onChange={(e) => {
                if (e.target.checked && !window.confirm(SWEAR_WARNING)) return;
                void update({ allowSwearing: e.target.checked });
              }}
            />{" "}
            Swear back if I do (18+)
          </label>
          <p className="faint small" style={{ margin: "4px 0" }}>
            {learned || "Still learning."} BYTE keeps numbers about how you write, never your messages, and only from chats that aren't private.
          </p>
          <button className="btn sm ghost" onClick={() => void api.styleReset().then(() => setLearned("Still learning (0 of 40 messages so far)."))}>
            Forget what it learned
          </button>
        </div>
      )}
    </div>
  );
}

/** Settings → About → Personality: how BYTE talks (sliders and presets; prompt.rs). */
function PersonalitySection() {
  const p = useStore((s) => s.settings?.personality) ?? BALANCED;
  const update = useStore((s) => s.updateSettings);
  const newChat = useStore((s) => s.newChat);
  const send = useStore((s) => s.send);
  const openSettings = useStore((s) => s.openSettings);
  const current = presetOf(p);
  return (
    <div className="section">
      <h4>Personality</h4>
      <p className="faint small" style={{ marginTop: 0 }}>How BYTE talks to you. Balanced is BYTE's usual voice; answers stay accurate whatever you pick.</p>
      <TalkModes />
      <div className="chips" role="group" aria-label="Personality presets">
        {PRESETS.map((x) => (
          <button key={x.id} className="chip" aria-pressed={current === x.id} title={x.hint} onClick={() => void update({ personality: x.p })}>
            {x.name}
          </button>
        ))}
        {!current && <span className="chip" aria-pressed>Your own</span>}
      </div>
      <div className="personality-sliders">
        {SLIDERS.map(({ key, label, low, high }) => (
          <label key={key} className="personality-slider">
            <span>
              {label} <span className="faint small">· {sliderWord(p[key], low, high)}</span>
            </span>
            <span className="faint small" style={{ textAlign: "right" }}>{low}</span>
            <input type="range" min={1} max={5} step={1} value={p[key]} aria-label={label} onChange={(e) => void update({ personality: { ...p, [key]: Number(e.target.value) } })} />
            <span className="faint small">{high}</span>
          </label>
        ))}
      </div>
      <button
        className="btn sm ghost"
        style={{ marginTop: 8 }}
        onClick={() => {
          openSettings(null);
          newChat(true);
          void send("In a few sentences: should I learn to cook or keep ordering takeout?");
        }}
      >
        Try it (in a private chat)
      </button>
    </div>
  );
}

/** Settings → Control: whether BYTE acts on what you say or type, or can simply talk (intentgate.rs). */
function ActionModeRow() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const mode = settings?.actionMode ?? "smart";
  return (
    <div className="field">
      <label>
        <span>How BYTE handles commands</span>
        <small>
          {mode === "always"
            ? "Everything that matches a command runs, even when you were only talking about it."
            : mode === "chat"
              ? "BYTE never acts on its own. Start a message with “do:” to make it do something, like “do: set the volume to 30”."
              : "BYTE acts on clear commands (“set the volume to 30”, “open Spotify”) and just talks about everything else (“how do I turn off the wifi?”, “what do you think of this?”)."}
        </small>
      </label>
      <select value={mode} aria-label="How BYTE handles commands" onChange={(e) => void update({ actionMode: e.target.value as "smart" | "always" | "chat" })}>
        <option value="smart">Smart: act on clear commands, otherwise talk</option>
        <option value="always">Always: anything that matches runs</option>
        <option value="chat">Chat only: act only after “do:”</option>
      </select>
    </div>
  );
}
