import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { documentDir, join } from "@tauri-apps/api/path";
import { CalendarPlus, Check, FileDown, Luggage, Plane, Wallet } from "lucide-react";
import { useState } from "react";

import { api } from "../../lib/api";
import { DOC_THEMES, fileName } from "../../lib/docs/spec";
import { tripIcs } from "../../lib/ics";
import { dayLabel, money, tripDocSpec, tripTotals } from "../../lib/trip";
import type { Source, TripPlan } from "../../lib/types";

type Tab = number | "budget" | "packing";

const b64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));

/** A trip plan: day tabs, budget, packing list, with Save as PDF and Add to Calendar. */
export function TripCard({ plan, sources }: { plan: TripPlan; sources?: Source[] }) {
  const [tab, setTab] = useState<Tab>(0);
  const [packed, setPacked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState<"pdf" | "ics" | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { total, left } = tripTotals(plan);
  const ics = tripIcs(plan);
  const cite = (n: number) => sources?.find((s) => s.n === n);

  const saveFile = async (kind: "pdf" | "ics") => {
    setError(null);
    setDone(null);
    try {
      const spec = tripDocSpec(plan, sources);
      const dir = await join(await documentDir(), "BYTE");
      const name = kind === "pdf" ? fileName(spec.title, "pdf") : fileName(spec.title, "pdf").replace(/\.pdf$/, ".ics");
      const dest = await saveDialog({ defaultPath: await join(dir, name) });
      if (!dest) return;
      setBusy(kind);
      if (kind === "pdf") {
        const { renderDoc } = await import("../../lib/docs/render");
        const { renderCharts } = await import("../../lib/docs/charts");
        const theme = DOC_THEMES[0];
        await api.docSave(dest, await renderDoc("pdf", spec, theme, await renderCharts(spec, theme).catch(() => ({}))));
      } else {
        // Opening the file hands it to Calendar, which asks where to add the events.
        await api.calendarOpen(dest, b64(ics!));
      }
      setDone(kind === "pdf" ? "Saved the PDF" : "Opened in Calendar");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const day = typeof tab === "number" ? plan.days[tab] : null;
  return (
    <div className="trip" role="region" aria-label={`Trip to ${plan.destination}`}>
      <div className="trip-head">
        <Plane size={15} />
        <span className="trip-title">
          {plan.days.length} {plan.days.length === 1 ? "day" : "days"} in {plan.destination}
        </span>
        <span className="hint">
          {plan.days[0]?.date ? `${dayLabel(plan.days[0].date)} – ${dayLabel(plan.days[plan.days.length - 1].date)}` : (plan.month ?? "")}
          {plan.travelers > 1 ? ` · ${plan.travelers} people` : ""}
        </span>
        <span className="spacer" />
        <button className="btn sm ghost" disabled={!!busy} onClick={() => void saveFile("pdf")}>
          <FileDown size={13} /> {busy === "pdf" ? "Saving…" : "Save as PDF"}
        </button>
        {ics && (
          <button className="btn sm ghost" disabled={!!busy} onClick={() => void saveFile("ics")} title="Save a calendar file and open it in Calendar">
            <CalendarPlus size={13} /> Add to Calendar
          </button>
        )}
      </div>
      {plan.weather && <div className="trip-weather">{plan.weather.split("\n")[0]}</div>}
      <div className="trip-tabs" role="tablist">
        {plan.days.map((d, i) => (
          <button key={i} role="tab" aria-selected={tab === i} className={tab === i ? "on" : ""} onClick={() => setTab(i)}>
            Day {i + 1}
            {d.date && <small>{dayLabel(d.date)}</small>}
          </button>
        ))}
        {plan.costs.length > 0 && (
          <button role="tab" aria-selected={tab === "budget"} className={tab === "budget" ? "on" : ""} onClick={() => setTab("budget")}>
            <Wallet size={13} /> Budget
          </button>
        )}
        {plan.packing.length > 0 && (
          <button role="tab" aria-selected={tab === "packing"} className={tab === "packing" ? "on" : ""} onClick={() => setTab("packing")}>
            <Luggage size={13} /> Packing
          </button>
        )}
      </div>
      <div className="trip-body">
        {day && (
          <>
            <div className="trip-day-title">{day.title}</div>
            <ol className="trip-items">
              {day.items.map((it, j) => (
                <li key={j}>
                  <span className="t">{it.time}</span>
                  <span className="what">
                    <b>{it.title}</b>
                    {it.place && <span className="where"> · {it.place}</span>}
                    {it.note && <span className="note">{it.note}</span>}
                  </span>
                  <span className="cost">
                    {it.cost != null ? money(it.cost, plan.currency) : "Free"}
                    {it.sources.map((n) => (
                      <sup key={n} className="cite-n" title={cite(n)?.title}>
                        {n}
                      </sup>
                    ))}
                  </span>
                </li>
              ))}
            </ol>
          </>
        )}
        {tab === "budget" && (
          <table className="trip-budget">
            <tbody>
              {plan.costs.map((c) => (
                <tr key={c.category}>
                  <td>{c.category}</td>
                  <td>{money(c.amount, plan.currency)}</td>
                </tr>
              ))}
              <tr className="total">
                <td>Estimated total</td>
                <td>{money(total, plan.currency)}</td>
              </tr>
              {left != null && (
                <tr className={left >= 0 ? "under" : "over"}>
                  <td>{left >= 0 ? "Left of your budget" : "Over your budget"}</td>
                  <td>{money(Math.abs(left), plan.currency)}</td>
                </tr>
              )}
            </tbody>
          </table>
        )}
        {tab === "packing" && (
          <ul className="trip-packing">
            {plan.packing.map((p, i) => (
              <li key={i}>
                <label>
                  <input
                    type="checkbox"
                    checked={packed.has(i)}
                    onChange={() => setPacked((s) => { const n = new Set(s); if (n.has(i)) n.delete(i); else n.add(i); return n; })}
                  />
                  {p}
                </label>
              </li>
            ))}
          </ul>
        )}
      </div>
      {(done || error) && (
        <div className={`trip-status ${error ? "error" : ""}`}>
          {error ? error : (
            <>
              <Check size={13} /> {done}
            </>
          )}
        </div>
      )}
    </div>
  );
}
