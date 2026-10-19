import { Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { api, errorText } from "../../lib/api";
import { bytes } from "../../lib/format";
import { fitSentence, PLACEMENT_LABEL, placementClass } from "../../lib/imageFit";
import type { ImageModel } from "../../lib/types";
import { useStore } from "../../state/store";

type Sort = "best" | "fastest" | "smallest" | "popular";

export const IMAGE_FILTERS: { id: string; label: string; hint?: string }[] = [
  { id: "realism", label: "Realism", hint: "Photo-like people, places and objects" },
  { id: "cartoon", label: "Cartoon", hint: "Cartoon, anime and flat colour styles" },
  { id: "illustration", label: "Illustration", hint: "Paintings, concept art and book art" },
  { id: "poster", label: "Poems and posters", hint: "Good for art behind text: poem posters, covers, invitations" },
  { id: "text", label: "Words in pictures", hint: "Better at spelling out words inside the image" },
  { id: "fast", label: "Fast" },
  { id: "lowvram", label: "Small graphics card", hint: "Runs on 6 GB or less of video memory" },
];


/** Quality of the version recommended for this PC (quant losses included). */
function headline(m: ImageModel): number {
  return m.quality - (m.variants[m.best]?.qualityCost ?? 0);
}

/** Sorts and filters the image catalog. Pure, so it is tested without the UI. */
export function arrange(models: ImageModel[], opts: { query: string; filter: string | null; sort: Sort; community: boolean; commercialOnly: boolean; kids?: boolean }): ImageModel[] {
  const q = opts.query.trim().toLowerCase();
  // Community models need their own switch; adult ones are never shown in kids mode.
  let list = models.filter((m) => (opts.community || !m.community) && !(opts.kids && m.mature));
  if (opts.commercialOnly) list = list.filter((m) => m.commercialOk);
  if (opts.filter) list = list.filter((m) => m.tags.includes(opts.filter!));
  if (q) list = list.filter((m) => [m.name, m.family, m.tagline, m.strengths, m.tags.join(" ")].join(" ").toLowerCase().includes(q));
  const by: Record<Sort, (a: ImageModel, b: ImageModel) => number> = {
    best: (a, b) => headline(b) - headline(a),
    fastest: (a, b) => a.steps - b.steps || headline(b) - headline(a),
    smallest: (a, b) => a.totalBytes - b.totalBytes,
    popular: (a, b) => b.downloads - a.downloads,
  };
  return [...list].sort(by[opts.sort]);
}

/**
 * Image Studio's model list (Windows builds): what each free image model is good at and whether this
 * PC's graphics card can run it. Downloading and making pictures arrive in the next builds.
 */
export function ImageCatalog() {
  const [models, setModels] = useState<ImageModel[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<string | null>(null);
  const [sort, setSort] = useState<Sort>("best");
  const [community, setCommunity] = useState(false);
  const [commercialOnly, setCommercialOnly] = useState(false);
  const [pick, setPick] = useState<Record<string, number>>({});
  const [fitOnly, setFitOnly] = useState(false);
  const kids = useStore((s) => !!s.settings?.kidsMode);

  useEffect(() => {
    api.imageCatalog().then(setModels).catch((e) => setError(errorText(e)));
  }, []);

  const shown = useMemo(() => arrange(models ?? [], { query, filter, sort, community, commercialOnly, kids }).filter((m) => !fitOnly || m.variants.some((v) => v.fit.placement === "vram")), [models, query, filter, sort, community, commercialOnly, kids, fitOnly]);

  return (
    <>
      <p className="faint" style={{ marginTop: 0 }}>
        Free image models, best first. Everything runs on this PC; pictures and photos never leave it. Downloading and making pictures are coming in the next test builds.
      </p>
      <div className="catalog-tools">
        <label className="search">
          <Search size={14} className="faint" />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search image models (e.g. realistic, fast, FLUX)" aria-label="Search image models" />
        </label>
        <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="Sort">
          <option value="best">Best to least</option>
          <option value="fastest">Fewest steps (fastest)</option>
          <option value="popular">Most downloaded</option>
          <option value="smallest">Smallest download</option>
        </select>
      </div>
      <div className="filters">
        <div className="chips" role="group" aria-label="Style">
          <button className="chip" aria-pressed={filter === null} onClick={() => setFilter(null)}>All</button>
          {IMAGE_FILTERS.map((f) => (
            <button key={f.id} className="chip" title={f.hint} aria-pressed={filter === f.id} onClick={() => setFilter(filter === f.id ? null : f.id)}>
              {f.label}
            </button>
          ))}
        </div>
        <div className="chips" role="group" aria-label="Options">
          <button className="chip" aria-pressed={fitOnly} onClick={() => setFitOnly(!fitOnly)} title="Only models with a version that fits entirely in your graphics card's memory">
            Fits my graphics card
          </button>
          <button className="chip" aria-pressed={commercialOnly} onClick={() => setCommercialOnly(!commercialOnly)} title="Hide models whose licence does not allow selling what you make">
            Commercial use OK
          </button>
          <button className="chip" aria-pressed={community} onClick={() => setCommunity(!community)} title="Models made by the community rather than the original labs. Check each licence.">
            Community models
          </button>
        </div>
      </div>
      {error && <div className="banner danger">{error}</div>}
      {models && models.length === 0 && <p className="faint">Image models are only in the Windows build for now.</p>}
      {models && models.length > 0 && shown.length === 0 && <p className="faint">No image models match these filters.</p>}
      <div className="model-list" style={{ marginTop: 12 }}>
        {shown.map((m) => {
          const vi = pick[m.id] ?? m.best;
          const v = m.variants[vi];
          return (
            <div key={m.id} className="model-card">
              <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                <b className="grow">{m.name}</b>
                <span className="faint">{m.family}</span>
                <span className={`pill ${placementClass(v.fit.placement)}`}>{PLACEMENT_LABEL[v.fit.placement]}</span>
              </div>
              <div>{m.tagline}</div>
              <div className="faint" style={{ fontSize: "0.9em" }}>{m.strengths}</div>
              <div className="row" style={{ gap: 8, flexWrap: "wrap" }} role="group" aria-label={`Version of ${m.name}`}>
                {m.variants.map((x, i) => (
                  <button
                    key={x.quant}
                    className="chip"
                    aria-pressed={i === vi}
                    onClick={() => setPick({ ...pick, [m.id]: i })}
                    title={`${x.quant}: ${bytes(x.sizeBytes)}${x.qualityCost ? `, about ${x.qualityCost} points less quality` : ", best quality"}`}
                  >
                    {x.quant} · {bytes(x.sizeBytes)}{i === m.best ? " ★" : ""}
                  </button>
                ))}
              </div>
              <div className={placementClass(v.fit.placement) === "danger" ? "banner danger" : "faint"} style={{ fontSize: "0.9em" }}>
                {fitSentence(v)}{vi === m.best ? " Recommended for this PC." : ""}
              </div>
              <div className="faint row" style={{ gap: 12, flexWrap: "wrap", fontSize: "0.85em" }}>
                <span>{bytes(v.sizeBytes + m.companions.reduce((n, f) => n + f.size, 0))} to download with its text files</span>
                <span>{m.steps} steps</span>
                <span>{m.license}{m.commercialOk || /selling/.test(m.license) ? "" : " · check before selling pictures"}</span>
                {m.community && <span>Community model by {m.source.split("/")[0]}</span>}
                {m.mature && <span>Adult content possible</span>}
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
}
