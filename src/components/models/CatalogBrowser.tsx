import { ask } from "@tauri-apps/plugin-dialog";
import { ChevronRight, Cpu, MemoryStick, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";

import { api, errorText } from "../../lib/api";
import { bytes, ramSize } from "../../lib/format";
import { displayName, fitGroup, RAM_TIERS, TAG_LABELS } from "../../lib/models";
import type { ModelStatus } from "../../lib/types";
import { useStore } from "../../state/store";
import { ModelCard } from "./ModelCard";

const CAPABILITIES = ["reasoning", "coding", "writing", "multilingual", "fast", "small"] as const;

/**
 * The model catalog: what this Mac can run, grouped by fit, with filters by
 * memory size and strength. Model files download on demand.
 */
export function CatalogBrowser() {
  const models = useStore((s) => s.models);
  const downloads = useStore((s) => s.downloads);
  const settings = useStore((s) => s.settings);
  const system = useStore((s) => s.system);
  const refresh = useStore((s) => s.refreshModels);
  const recommended = useStore((s) => s.recommended);
  const [error, setError] = useState<string | null>(null);
  const [tier, setTier] = useState<number | "mine">("mine");
  const [cap, setCap] = useState<string | null>(null);
  const [showBig, setShowBig] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

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
    onDelete: async (k: string) => {
      if (await ask(`Delete ${displayName(models, k, true)}? You can download it again later.`, { title: "Delete model", kind: "warning" })) {
        await run(() => api.modelDelete(k));
      }
    },
  };

  const chat = useMemo(() => {
    let list = models.filter((m) => m.role === "chat");
    if (cap) list = list.filter((m) => m.tags.includes(cap));
    if (tier !== "mine") list = list.filter((m) => m.minRamGb <= tier);
    // Rank by the quality of the version that fits this Mac (a 2-bit squeeze of
    // a big model can be worse than a small model at full quality).
    const score = (m: ModelStatus) => m.variants.find((v) => v.key === m.best)?.quality ?? m.quality - 100;
    return [...list].sort((a, b) => score(b) - score(a));
  }, [models, cap, tier]);

  const groups: { id: string; title: string; hint: string; items: ModelStatus[] }[] =
    tier === "mine"
      ? [
          { id: "great", title: "Runs great on this Mac", hint: "Fast, with room for your other apps.", items: chat.filter((m) => fitGroup(m) === "great") },
          { id: "tight", title: "Runs on this Mac", hint: "Close other heavy apps for best speed.", items: chat.filter((m) => fitGroup(m) === "tight") },
          { id: "toobig", title: "Needs a Mac with more memory", hint: "Shown so you know what bigger Macs can run.", items: chat.filter((m) => fitGroup(m) === "toobig") },
        ]
      : [{ id: "all", title: `Models for Macs with ${tier} GB or less`, hint: "Filtered by the memory they need.", items: chat }];

  const helpers = models.filter((m) => m.role !== "chat");

  return (
    <>
      <div className="mac-strip">
        <span><Cpu size={15} /> {system?.chip ?? "This Mac"}</span>
        <span><MemoryStick size={15} /> {system ? ramSize(system.totalRamBytes) : "?"} memory</span>
        <span className="faint">{system ? `${bytes(system.freeDiskBytes)} free disk` : ""}</span>
        <button
          className="btn sm ghost"
          style={{ marginLeft: "auto" }}
          disabled={refreshing}
          onClick={async () => {
            setRefreshing(true);
            await run(() => api.catalogRefresh());
            setRefreshing(false);
          }}
          title="Check for new models"
        >
          <RefreshCw size={13} className={refreshing ? "spin" : undefined} /> Check for new models
        </button>
      </div>

      <div className="filters">
        <div className="chips" role="group" aria-label="Memory">
          <button className="chip" aria-pressed={tier === "mine"} onClick={() => setTier("mine")}>This Mac</button>
          {RAM_TIERS.map((t) => (
            <button key={t} className="chip" aria-pressed={tier === t} onClick={() => setTier(t)}>{t} GB</button>
          ))}
        </div>
        <div className="chips" role="group" aria-label="Strength">
          <button className="chip" aria-pressed={cap === null} onClick={() => setCap(null)}>All</button>
          {CAPABILITIES.map((c) => (
            <button key={c} className="chip" aria-pressed={cap === c} onClick={() => setCap(cap === c ? null : c)}>{TAG_LABELS[c]}</button>
          ))}
        </div>
      </div>

      {error && <div className="banner danger">{error}</div>}

      {groups.map((g) =>
        g.items.length === 0 ? null : (
          <div className="section" key={g.id}>
            {g.id === "toobig" ? (
              <button className="section-toggle" onClick={() => setShowBig(!showBig)} aria-expanded={showBig}>
                <ChevronRight size={14} style={{ transform: showBig ? "rotate(90deg)" : undefined }} />
                <h4 style={{ margin: 0 }}>{g.title} ({g.items.length})</h4>
              </button>
            ) : (
              <h4>{g.title}</h4>
            )}
            {(g.id !== "toobig" || showBig) && (
              <>
                <p className="faint" style={{ margin: "0 0 8px", fontSize: "0.88em" }}>{g.hint}</p>
                <div className="model-list">
                  {g.items.map((m) => (
                    <ModelCard key={m.id} model={m} {...handlers} />
                  ))}
                </div>
              </>
            )}
          </div>
        ),
      )}

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
    </>
  );
}
