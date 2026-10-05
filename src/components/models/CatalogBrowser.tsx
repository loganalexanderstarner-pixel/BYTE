import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronRight, Cpu, HardDrive, Layers, MemoryStick, RefreshCw, Search, Sparkles } from "lucide-react";
import { useMemo, useState } from "react";

import { api, errorText } from "../../lib/api";
import { bytes, ramSize } from "../../lib/format";
import { COMMUNITY_CAPS, displayName, fitGroup, RAM_TIERS, TAG_LABELS } from "../../lib/models";
import type { ModelStatus, VariantStatus } from "../../lib/types";
import { useStore } from "../../state/store";
import { ModelCard } from "./ModelCard";
import { osText } from "../../lib/platform";

const CAPABILITIES = ["reasoning", "coding", "writing", "multilingual", "fast", "small", "moe", "community", "stories", "uncensored"] as const;
const PAGE = 20;

type Sort = "best" | "newest" | "smallest" | "fastest";

/** The version shown for a model when sorting: its best fit, else its smallest. */
function headline(m: ModelStatus): VariantStatus {
  return m.variants.find((v) => v.key === m.best) ?? m.variants[0];
}

/**
 * The model catalog: what this Mac can run, grouped by fit, with search,
 * sorting and filters. Model files download on demand; downloaded versions
 * can be deleted any time.
 */
export function CatalogBrowser() {
  const models = useStore((s) => s.models);
  const downloads = useStore((s) => s.downloads);
  const settings = useStore((s) => s.settings);
  const system = useStore((s) => s.system);
  const refresh = useStore((s) => s.refreshModels);
  const recommended = useStore((s) => s.recommended);
  const loaded = useStore((s) => s.loaded);
  const refreshLoaded = useStore((s) => s.refreshLoaded);
  const [error, setError] = useState<string | null>(null);
  const [tier, setTier] = useState<number | "mine" | "downloaded">("mine");
  const [cap, setCap] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("best");
  const [showBig, setShowBig] = useState(false);
  const [limit, setLimit] = useState<Record<string, number>>({});
  const [refreshing, setRefreshing] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const run = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    await refresh();
  };

  const handlers = {
    downloads,
    recommended,
    activeKey: settings?.activeModel,
    onDownload: (k: string) => run(() => api.modelDownload(k)),
    onPause: (k: string) => run(() => api.modelPause(k)),
    onActivate: (k: string) => run(() => api.modelActivate(k)),
    loaded,
    onLoad: (k: string) => run(async () => {
      const load = api.modelLoad(k);
      // Show "Loading…" right away; the load itself can take a while.
      setTimeout(() => void refreshLoaded(), 300);
      await load;
    }),
    onUnload: (k: string) => run(() => api.modelUnload(k)),
    onDelete: async (k: string) => {
      if (await ask(`Delete ${displayName(models, k, true)}? You can download it again later.`, { title: "Delete model", kind: "warning" })) {
        await run(() => api.modelDelete(k));
      }
    },
  };

  const installed = useMemo(() => models.flatMap((m) => m.variants.filter((v) => v.installed)), [models]);
  const installedBytes = installed.reduce((s, v) => s + v.sizeBytes, 0);
  const chatCount = models.filter((m) => m.role === "chat").length;
  const versionCount = models.filter((m) => m.role === "chat").reduce((s, m) => s + m.variants.length, 0);

  const chat = useMemo(() => {
    let list = models.filter((m) => m.role === "chat");
    const q = query.trim().toLowerCase();
    if (q) {
      list = list.filter((m) =>
        [m.name, m.family ?? "", m.tagline, m.usedFor ?? "", m.tags.join(" "), m.repo, m.details?.about ?? "", m.details?.author ?? ""].join(" ").toLowerCase().includes(q),
      );
    }
    if (cap) list = list.filter((m) => m.tags.includes(cap));
    // Community fine-tunes show under their own filters or when searching, not in the main list.
    if (!q && !(cap && COMMUNITY_CAPS.includes(cap))) list = list.filter((m) => !m.tags.includes("community"));
    if (typeof tier === "number") list = list.filter((m) => m.minRamGb <= tier);
    if (tier === "downloaded") list = list.filter((m) => m.variants.some((v) => v.installed || v.partialBytes > 0));
    const bySort: Record<Sort, (a: ModelStatus, b: ModelStatus) => number> = {
      // Quality of the version that fits this Mac, adjusted for speed.
      best: (a, b) => headline(b).quality - headline(a).quality,
      newest: (a, b) => (b.released ?? "").localeCompare(a.released ?? ""),
      smallest: (a, b) => a.variants[0].sizeBytes - b.variants[0].sizeBytes,
      fastest: (a, b) => headline(b).speed.tokensPerSec - headline(a).speed.tokensPerSec,
    };
    return [...list].sort(bySort[sort]);
  }, [models, cap, tier, query, sort]);

  const groups: { id: string; title: string; hint: string; items: ModelStatus[] }[] =
    tier === "mine"
      ? [
          { id: "great", title: osText("Runs great on this Mac"), hint: "Fast, with room for your other apps.", items: chat.filter((m) => fitGroup(m) === "great") },
          { id: "tight", title: osText("Runs on this Mac"), hint: "Close other heavy apps for best speed.", items: chat.filter((m) => fitGroup(m) === "tight") },
          { id: "toobig", title: osText("Needs a Mac with more memory"), hint: osText("Shown so you know what bigger Macs can run."), items: chat.filter((m) => fitGroup(m) === "toobig") },
        ]
      : tier === "downloaded"
        ? [{ id: "dl", title: "Downloaded models", hint: "Delete versions you no longer use to free up disk space.", items: chat }]
        : [{ id: "all", title: osText(`Models for Macs with ${tier} GB or less`), hint: "Filtered by the memory they need.", items: chat }];

  const helpers = models.filter((m) => m.role !== "chat");
  const chip = system?.chipInfo;

  return (
    <>
      <div className="mac-strip">
        <span><Cpu size={15} /> {chip?.name ?? system?.chip ?? osText("This Mac")}{chip?.gpuCores ? ` · ${chip.gpuCores}-core GPU` : ""}</span>
        <span><MemoryStick size={15} /> {system ? ramSize(system.totalRamBytes) : "?"}</span>
        {chip && chip.neuralEngineTops > 0 && (
          <span className="faint" title="The Neural Engine powers on-device OCR and voice. Chat models run on the GPU.">
            <Sparkles size={14} /> Neural Engine {chip.neuralEngineTops} TOPS
          </span>
        )}
        <button
          className="btn sm ghost"
          style={{ marginLeft: "auto" }}
          disabled={refreshing}
          onClick={async () => {
            setRefreshing(true);
            setNotice(null);
            try {
              const changed = await api.catalogRefresh();
              setNotice(changed ? "Model list updated." : "You already have the newest model list.");
            } catch (e) {
              setNotice(errorText(e));
            }
            await refresh();
            setRefreshing(false);
          }}
          title="Check for new models"
        >
          <RefreshCw size={13} className={refreshing ? "spin" : undefined} /> Check for new models
        </button>
      </div>

      {loaded.length > 0 && (
        <div className="loaded-strip">
          <div className="row" style={{ gap: 8 }}>
            <Layers size={15} style={{ color: "var(--accent)" }} />
            <b>In memory now</b>
            <span className="faint">
              · {bytes(loaded.reduce((s, l) => s + l.neededBytes, 0))} of {system ? ramSize(system.totalRamBytes) : "?"}
            </span>
          </div>
          {loaded.map((l) => (
            <div key={l.key} className="loaded-row">
              <span className="grow">
                {displayName(models, l.key, true)}
                <span className="faint">
                  {" "}· {l.primary ? "main" : "alongside"} · {bytes(l.neededBytes)} · {Math.round(l.context / 1024)}k context
                  {l.status.state === "starting" ? " · loading…" : l.status.state === "error" ? " · failed" : ""}
                </span>
              </span>
              {!l.primary && (
                <button className="btn sm ghost" onClick={() => handlers.onUnload(l.key)}>Unload</button>
              )}
            </div>
          ))}
          {loaded.length === 1 && (
            <p className="faint" style={{ margin: "4px 0 0", fontSize: "0.85em" }}>
              Tip: downloaded models that fit in the memory left show <b>Load alongside</b>. Then choose which one answers in the chat box, or compare their answers side by side.
            </p>
          )}
        </div>
      )}

      <div className="storage-bar">
        <HardDrive size={15} style={{ color: "var(--accent)" }} />
        <span className="grow">
          {installed.length
            ? `${installed.length} downloaded version${installed.length === 1 ? "" : "s"} using ${bytes(installedBytes)}`
            : "No models downloaded yet"}
          {system && <span className="faint"> · {bytes(system.freeDiskBytes)} free</span>}
        </span>
        <span className="faint">{chatCount} models · {versionCount} versions in the catalog</span>
      </div>

      <div className="catalog-tools">
        <label className="search">
          <Search size={14} className="faint" />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search models (e.g. coder, gemma, 27B)" aria-label="Search models" />
        </label>
        <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort">
          <option value="best">{osText("Best for this Mac")}</option>
          <option value="newest">Newest</option>
          <option value="smallest">Smallest</option>
          <option value="fastest">Fastest</option>
        </select>
      </div>

      <div className="filters">
        <div className="chips" role="group" aria-label="Memory">
          <button className="chip" aria-pressed={tier === "mine"} onClick={() => setTier("mine")}>{osText("This Mac")}</button>
          <button className="chip" aria-pressed={tier === "downloaded"} onClick={() => setTier("downloaded")}>Downloaded ({installed.length})</button>
          {RAM_TIERS.map((t) => (
            <button key={t} className="chip" aria-pressed={tier === t} onClick={() => setTier(t)}>{t} GB</button>
          ))}
        </div>
        <div className="chips" role="group" aria-label="Strength">
          <button className="chip" aria-pressed={cap === null} onClick={() => setCap(null)}>All</button>
          {CAPABILITIES.map((c) => (
            <button key={c} className="chip" aria-pressed={cap === c} onClick={() => setCap(cap === c ? null : c)} title={c === "moe" ? "Mixture-of-experts: big-model knowledge, small-model speed" : c === "community" ? "Fine-tunes and merges made by the community: stories, role-play, uncensored versions" : c === "uncensored" ? "Safety tuning removed: refuses less, can say things other models won't" : undefined}>
              {TAG_LABELS[c]}
            </button>
          ))}
        </div>
      </div>

      {notice && <div className="banner">{notice}</div>}
      {error && <div className="banner danger">{error}</div>}
      {chat.length === 0 && <p className="faint" style={{ marginTop: 16 }}>No models match these filters.</p>}

      {groups.map((g) => {
        if (g.items.length === 0) return null;
        const shown = limit[g.id] ?? PAGE;
        const open = g.id !== "toobig" || showBig;
        return (
          <div className="section" key={g.id}>
            {g.id === "toobig" ? (
              <button className="section-toggle" onClick={() => setShowBig(!showBig)} aria-expanded={showBig}>
                <ChevronRight size={14} style={{ transform: showBig ? "rotate(90deg)" : undefined }} />
                <h4 style={{ margin: 0 }}>{g.title} ({g.items.length})</h4>
              </button>
            ) : (
              <h4>{g.title} ({g.items.length})</h4>
            )}
            {open && (
              <>
                <p className="faint" style={{ margin: "0 0 8px", fontSize: "0.88em" }}>{g.hint}</p>
                <div className="model-list">
                  {g.items.slice(0, shown).map((m) => (
                    <ModelCard key={m.id} model={m} {...handlers} />
                  ))}
                </div>
                {g.items.length > shown && (
                  <button className="btn sm ghost" style={{ marginTop: 8 }} onClick={() => setLimit({ ...limit, [g.id]: shown + PAGE })}>
                    Show {Math.min(PAGE, g.items.length - shown)} more
                  </button>
                )}
              </>
            )}
          </div>
        );
      })}

      {tier === "mine" && !query && !cap && (
        <div className="section">
          <h4>Helper models</h4>
          <p className="faint" style={{ marginTop: 0, fontSize: "0.88em" }}>
            Optional small models that upcoming features use (faster replies, knowledge-base search).
          </p>
          <div className="model-list">
            {helpers.map((m) => (
              <ModelCard key={m.id} model={m} {...handlers} onActivate={undefined} />
            ))}
          </div>
        </div>
      )}
    </>
  );
}
