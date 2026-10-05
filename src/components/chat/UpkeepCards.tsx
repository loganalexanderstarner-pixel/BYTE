import { CircleAlert, CircleCheck, FolderSearch, HardDrive, Info, Settings, Stethoscope, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { useMemo, useState } from "react";

import { api, errorText } from "../../lib/api";
import type { Health, Storage, Trashed } from "../../lib/types";
import { sizeText, treemap, usedPercent } from "../../lib/upkeep";
import { osText } from "../../lib/platform";

type TrashState = { phase: "idle" } | { phase: "confirm" } | { phase: "busy" } | { phase: "done"; result: Trashed; undone?: boolean } | { phase: "error"; message: string };

/** Move-to-Trash with a confirm step and Undo (Put Back). Only ids from BYTE's scan. */
function TrashButton({ scanId, id, what }: { scanId: string; id: string; what: string }) {
  const [s, setS] = useState<TrashState>({ phase: "idle" });
  const go = async () => {
    setS({ phase: "busy" });
    try {
      const result = await api.upkeepTrash(scanId, id);
      setS(result.moved === 0 && result.error ? { phase: "error", message: result.error } : { phase: "done", result });
    } catch (e) {
      setS({ phase: "error", message: errorText(e) });
    }
  };
  const undo = async (r: Trashed) => {
    if (!r.undo) return;
    try {
      if (await api.macUndo(r.undo)) setS({ phase: "done", result: r, undone: true });
    } catch (e) {
      setS({ phase: "error", message: errorText(e) });
    }
  };
  if (s.phase === "confirm")
    return (
      <span className="trash-confirm">
        Move {what} to the Trash?
        <button className="btn sm danger" onClick={() => void go()}>
          <Trash2 size={13} /> Move
        </button>
        <button className="btn sm ghost" onClick={() => setS({ phase: "idle" })} aria-label="Cancel">
          <X size={13} />
        </button>
      </span>
    );
  if (s.phase === "done")
    return s.undone ? (
      <span className="muted small">Put back</span>
    ) : (
      <span className="trash-done">
        <span className="small">
          Moved to the Trash{s.result.moved > 1 ? ` (${s.result.moved})` : ""}
          {s.result.error && <span className="hint danger"> · {s.result.error}</span>}
        </span>
        {s.result.undo && (
          <button className="btn sm ghost" onClick={() => void undo(s.result)} title="Put it back where it was">
            <Undo2 size={13} /> Undo
          </button>
        )}
      </span>
    );
  return (
    <span className="trash-idle">
      <button className="btn sm ghost" disabled={s.phase === "busy"} onClick={() => setS({ phase: "confirm" })} title="Moves to the Trash; nothing is deleted until you empty it">
        <Trash2 size={13} /> Move to Trash
      </button>
      {s.phase === "error" && <span className="hint danger">{s.message}</span>}
    </span>
  );
}

/** What's using the disk: a chart of the biggest folders, suggestions and big files. */
export function StorageCard({ storage }: { storage: Storage }) {
  const [error, setError] = useState<string | null>(null);
  const rects = useMemo(() => treemap(storage.folders.map((f) => f.bytes), 100, 44), [storage.folders]);
  const used = usedPercent(storage.total, storage.free);
  const max = storage.folders[0]?.bytes ?? 1;
  const reveal = (id: string) => void api.upkeepReveal(storage.scanId, id).catch((e) => setError(errorText(e)));
  return (
    <div className="shop-card upkeep-card" role="region" aria-label={osText("Storage on this Mac")}>
      <div className="shop-head">
        <HardDrive size={16} />
        <b>Storage</b>
        {used !== null && (
          <span className="muted">
            · {sizeText(storage.free)} free of {sizeText(storage.total)}
          </span>
        )}
      </div>
      {used !== null && (
        <div className={`disk-bar ${used >= 95 ? "bad" : used >= 88 ? "warn" : ""}`} role="meter" aria-valuenow={used} aria-valuemin={0} aria-valuemax={100} aria-label="Disk used">
          <span style={{ width: `${used}%` }} />
        </div>
      )}
      {storage.folders.length > 0 && (
        <div className="treemap" aria-label="Biggest folders">
          {storage.folders.map((f, i) => {
            const r = rects[i];
            if (!r || r.w * r.h === 0) return null;
            const share = Math.round(20 + (f.bytes / max) * 45);
            return (
              <div
                key={f.path + f.name}
                className="tile"
                style={{ left: `${r.x}%`, top: `${(r.y / 44) * 100}%`, width: `${r.w}%`, height: `${(r.h / 44) * 100}%`, background: `color-mix(in srgb, var(--accent) ${share}%, var(--surface))` }}
                title={`${f.name} (${f.path}): ${sizeText(f.bytes)}`}
              >
                {r.w > 9 && r.h > 6 && (
                  <span>
                    {f.name}
                    <small>{sizeText(f.bytes)}</small>
                  </span>
                )}
              </div>
            );
          })}
        </div>
      )}
      {storage.suggestions.length > 0 && (
        <ul className="upkeep-list">
          {storage.suggestions.map((s) => (
            <li key={s.id}>
              <div className="row">
                <b>{s.title}</b>
                <span className="size">{sizeText(s.bytes)}</span>
                {s.canTrash && <TrashButton scanId={storage.scanId} id={s.id} what={`${s.count} ${s.count === 1 ? "item" : "items"} (${sizeText(s.bytes)})`} />}
              </div>
              <div className="muted small">{s.why}</div>
              {s.items.length > 0 && (
                <details>
                  <summary className="small">
                    {s.count} {s.count === 1 ? "item" : "items"}
                  </summary>
                  <ul className="paths">
                    {s.items.map((it) => (
                      <li key={it} className="mono small">
                        {it}
                      </li>
                    ))}
                    {s.count > s.items.length && <li className="muted small">and {s.count - s.items.length} more</li>}
                  </ul>
                </details>
              )}
            </li>
          ))}
        </ul>
      )}
      {storage.big.length > 0 && (
        <details className="big-files">
          <summary>Biggest files ({storage.big.length})</summary>
          <ul className="upkeep-list">
            {storage.big.map((b) => (
              <li key={b.id}>
                <div className="row">
                  <span className="name" title={b.path}>
                    {b.name}
                  </span>
                  <span className="size">{sizeText(b.bytes)}</span>
                  <button className="icon-btn sm" onClick={() => reveal(b.id)} title={osText("Show in Finder")} aria-label={osText(`Show ${b.name} in Finder`)}>
                    <FolderSearch size={13} />
                  </button>
                  <TrashButton scanId={storage.scanId} id={b.id} what={b.name} />
                </div>
                <div className="muted small mono">
                  {b.path}
                  {b.daysOld !== null && b.daysOld >= 30 && ` · unchanged for ${b.daysOld >= 365 ? `${Math.floor(b.daysOld / 365)}+ years` : `${b.daysOld} days`}`}
                </div>
              </li>
            ))}
          </ul>
        </details>
      )}
      <p className="muted small">
        {storage.partial && "Your home folder is very large, so BYTE stopped early; real sizes are at least these. "}
        Things go to the Trash, so you can put them back until you empty it.
      </p>
      {error && <span className="hint danger">{error}</span>}
    </div>
  );
}

const LEVEL_ICON = { bad: CircleAlert, warn: TriangleAlert, ok: CircleCheck, info: Info } as const;

/** A Mac health check: what matters, with buttons for settings and busy apps. */
export function HealthCard({ health }: { health: Health }) {
  const [error, setError] = useState<string | null>(null);
  const [quit, setQuit] = useState<Record<string, "busy" | "done">>({});
  const openSettings = (url: string) => void api.upkeepOpenSettings(url).catch((e) => setError(errorText(e)));
  const quitApp = async (app: string) => {
    setQuit((q) => ({ ...q, [app]: "busy" }));
    try {
      await api.upkeepQuit(health.id, app);
      setQuit((q) => ({ ...q, [app]: "done" }));
    } catch (e) {
      setError(errorText(e));
      setQuit((q) => {
        const { [app]: _gone, ...rest } = q;
        return rest;
      });
    }
  };
  return (
    <div className="shop-card upkeep-card" role="region" aria-label={health.title}>
      <div className="shop-head">
        <Stethoscope size={16} />
        <b>{health.title}</b>
      </div>
      <ul className="health-list">
        {health.checks.map((c, i) => {
          const Icon = LEVEL_ICON[c.level];
          return (
            <li key={i} className={c.level}>
              <Icon size={15} className="level" aria-label={c.level} />
              <div>
                <div>
                  <b>{c.label}</b> <span className="value">{c.value}</span>
                </div>
                {c.tip && <div className="muted small">{c.tip}</div>}
              </div>
              {c.settings && (
                <button className="btn sm ghost" onClick={() => openSettings(c.settings!)} title="Open in System Settings">
                  <Settings size={13} /> {c.settingsLabel ?? "Settings"}
                </button>
              )}
            </li>
          );
        })}
      </ul>
      {health.procs.length > 0 && (
        <table className="procs">
          <thead>
            <tr>
              <th>Busiest now</th>
              <th>CPU</th>
              <th>Memory</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {health.procs.map((p) => (
              <tr key={p.name}>
                <td title={p.name}>{p.app ?? p.name}</td>
                <td className={p.cpu >= 50 ? "hot" : ""}>{p.cpu.toFixed(0)}%</td>
                <td>{p.mem}</td>
                <td>
                  {p.app &&
                    (quit[p.app] === "done" ? (
                      <span className="muted small">Asked to quit</span>
                    ) : (
                      <button className="btn sm ghost" disabled={quit[p.app] === "busy"} onClick={() => void quitApp(p.app!)} title={`Quit ${p.app} (it can still ask you to save)`}>
                        Quit
                      </button>
                    ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {error && <span className="hint danger">{error}</span>}
    </div>
  );
}
