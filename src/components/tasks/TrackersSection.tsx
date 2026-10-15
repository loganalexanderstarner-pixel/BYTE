import { openUrl } from "@tauri-apps/plugin-opener";
import { CalendarHeart, Check, ExternalLink, Package, Plus, Receipt, Trash2, Wrench } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { TRACKER_TABS, billTotals, blankTracker, everyText, inDays, isOverdue, moneyText, parseAmount } from "../../lib/trackers";
import type { Tracker, TrackerKind } from "../../lib/types";

const ICONS = { package: Package, bill: Receipt, event: CalendarHeart, upkeep: Wrench } as const;

/** What a tracker's line says under its name. */
function detail(t: Tracker, now: number): string {
  const when = inDays(t.next, now);
  switch (t.kind) {
    case "package":
      return [t.carrier, t.number, t.next ? `expected ${when}` : ""].filter(Boolean).join(" · ");
    case "bill":
      return [t.amount != null ? `${moneyText(t.amount, t.currency)} ${t.cycle === "yearly" ? "a year" : t.cycle === "weekly" ? "a week" : t.cycle === "quarterly" ? "a quarter" : "a month"}` : "", when && `next ${when}`].filter(Boolean).join(" · ");
    case "event":
      return [t.next ? new Date(`${t.next}T12:00`).toLocaleDateString(undefined, { month: "long", day: "numeric" }) : "", when, t.budget != null ? `budget ${moneyText(t.budget, t.currency)}` : "", t.ideas.length ? `ideas: ${t.ideas.join(", ")}` : ""]
        .filter(Boolean)
        .join(" · ");
    case "upkeep":
      return [`${everyText(t)}`, when && `due ${when}`, t.lastDone ? `last done ${inDays(t.lastDone, now)}` : ""].filter(Boolean).join(" · ");
  }
}

/** The add row for one kind. */
function AddRow({ kind, onAdded }: { kind: TrackerKind; onAdded: (e?: string) => void }) {
  const [t, setT] = useState<Tracker>(() => blankTracker(kind));
  const [amount, setAmount] = useState("");
  const [when, setWhen] = useState("");
  const [parsed, setParsed] = useState<string | null>(null);
  const [carrier, setCarrier] = useState<[string, string] | null>(null);

  useEffect(() => {
    const id = setTimeout(() => {
      if (!when.trim()) return setParsed(null);
      api.trackerDateParse(when).then(setParsed, () => setParsed(null));
    }, 200);
    return () => clearTimeout(id);
  }, [when]);
  useEffect(() => {
    if (kind !== "package") return;
    const id = setTimeout(() => {
      if (t.number.trim().length < 10) return setCarrier(null);
      api.trackerCarrier(t.number).then(setCarrier, () => setCarrier(null));
    }, 200);
    return () => clearTimeout(id);
  }, [kind, t.number]);

  const ready = kind === "package" ? !!carrier : kind === "bill" ? !!t.name.trim() && parseAmount(amount) != null : kind === "event" ? !!t.name.trim() && !!parsed : !!t.name.trim();
  const add = async () => {
    try {
      const next = kind === "upkeep" ? null : parsed;
      const lastDone = kind === "upkeep" ? parsed : null;
      await api.trackerSave({ ...t, amount: kind === "bill" ? parseAmount(amount) : null, next, lastDone });
      setT(blankTracker(kind));
      setAmount("");
      setWhen("");
      onAdded();
    } catch (e) {
      onAdded(errorText(e));
    }
  };
  const whenField = (placeholder: string, label: string) => (
    <input value={when} onChange={(e) => setWhen(e.target.value)} placeholder={placeholder} aria-label={label} className={when.trim() && !parsed ? "invalid" : ""} title={parsed ?? undefined} />
  );

  return (
    <form
      className="schedule-add tracker-add"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) void add();
      }}
    >
      {kind === "package" && (
        <>
          <input className="grow" value={t.number} onChange={(e) => setT({ ...t, number: e.target.value })} placeholder="Tracking number" aria-label="Tracking number" />
          <input value={t.name} onChange={(e) => setT({ ...t, name: e.target.value })} placeholder="What it is (optional)" aria-label="What it is" />
          {whenField("Expected (optional)", "Expected delivery")}
          {carrier && <span className="muted small">{carrier[0]}</span>}
        </>
      )}
      {kind === "bill" && (
        <>
          <input className="grow" value={t.name} onChange={(e) => setT({ ...t, name: e.target.value })} placeholder="Netflix, rent, car insurance…" aria-label="Bill or subscription" />
          <input className="target-input" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="$15.49" aria-label="Amount" />
          <select value={t.cycle} onChange={(e) => setT({ ...t, cycle: e.target.value as Tracker["cycle"] })} aria-label="How often">
            <option value="monthly">a month</option>
            <option value="yearly">a year</option>
            <option value="quarterly">a quarter</option>
            <option value="weekly">a week</option>
          </select>
          {whenField("Next due: the 12th", "Next due")}
        </>
      )}
      {kind === "event" && (
        <>
          <input className="grow" value={t.name} onChange={(e) => setT({ ...t, name: e.target.value })} placeholder="Sam's birthday" aria-label="Whose date" />
          {whenField("March 3", "Date")}
        </>
      )}
      {kind === "upkeep" && (
        <>
          <input className="grow" value={t.name} onChange={(e) => setT({ ...t, name: e.target.value })} placeholder="Change the furnace filter" aria-label="What needs doing" />
          <select
            value={t.everyMonths ? `m${t.everyMonths}` : `d${t.everyDays}`}
            onChange={(e) => {
              const v = e.target.value;
              setT({ ...t, everyMonths: v[0] === "m" ? Number(v.slice(1)) : null, everyDays: v[0] === "d" ? Number(v.slice(1)) : null });
            }}
            aria-label="How often"
          >
            {[["d7", "every week"], ["d14", "every 2 weeks"], ["m1", "every month"], ["m3", "every 3 months"], ["m6", "every 6 months"], ["m12", "every year"], ["m24", "every 2 years"]].map(([v, l]) => (
              <option key={v} value={v}>
                {l}
              </option>
            ))}
          </select>
          {whenField("Last done (optional)", "Last done")}
        </>
      )}
      <button className="btn sm primary" disabled={!ready}>
        <Plus size={14} /> Add
      </button>
    </form>
  );
}

/** Packages, bills and subscriptions, birthdays, and maintenance, in the ✅ panel (Rust `trackers.rs`). */
export function TrackersSection() {
  const [all, setAll] = useState<Tracker[]>([]);
  const [tab, setTab] = useState<TrackerKind>("bill");
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const load = useCallback(() => {
    api.trackersList().then(setAll, (e) => setError(errorText(e)));
    setNow(Date.now());
  }, []);
  useEffect(load, [load]);

  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    load();
  };
  const shown = all.filter((t) => t.kind === tab && !t.done);
  const totals = tab === "bill" ? billTotals(all) : null;
  const info = TRACKER_TABS.find((t) => t.kind === tab)!;
  const Icon = ICONS[tab];

  return (
    <>
      <div className="recipe-box-head schedules-head">
        <Receipt size={16} />
        <h3>Trackers</h3>
      </div>
      <div className="segmented tracker-tabs" role="tablist" aria-label="What to track">
        {TRACKER_TABS.map((t) => {
          const n = all.filter((x) => x.kind === t.kind && !x.done).length;
          return (
            <button key={t.kind} role="tab" aria-selected={tab === t.kind} onClick={() => setTab(t.kind)}>
              {t.label}
              {n > 0 && <span className="feed-new">{n}</span>}
            </button>
          );
        })}
      </div>
      {error && <div className="hint danger">{error}</div>}
      {totals && (
        <p className="small tracker-total">
          About <b>{moneyText(totals.month, totals.currency)} a month</b> · {moneyText(totals.year, totals.currency)} a year
        </p>
      )}
      {shown.length === 0 ? (
        <p className="muted small">{info.empty}</p>
      ) : (
        <ul className="schedule-list">
          {shown.map((t) => (
            <li key={t.id}>
              <Icon size={15} />
              <div className="grow">
                <b>{t.name}</b>
                <div className={`small ${isOverdue(t, now) && t.kind === "upkeep" ? "hint danger" : "muted"}`}>
                  {detail(t, now)}
                  {isOverdue(t, now) && t.kind === "upkeep" && " · overdue"}
                </div>
                {t.notes && <div className="small prompt">{t.notes}</div>}
              </div>
              {t.kind === "package" && t.link && (
                <button className="btn sm ghost" onClick={() => void openUrl(t.link)} title={`Track on ${t.carrier}'s site`}>
                  <ExternalLink size={13} /> Track
                </button>
              )}
              {t.kind !== "event" && (
                <button className="btn sm ghost" onClick={() => void guard(() => api.trackerDone(t.id))} title={t.kind === "upkeep" ? "Done today (the next date moves on)" : t.kind === "package" ? "It arrived" : "Cancelled or paid off: stop tracking"}>
                  <Check size={13} /> {t.kind === "upkeep" ? "Done today" : t.kind === "package" ? "Arrived" : "Stop"}
                </button>
              )}
              <button className="icon-btn sm" onClick={() => void guard(() => api.trackerDelete(t.id))} aria-label={`Delete ${t.name}`} title="Delete">
                <Trash2 size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <AddRow key={tab} kind={tab} onAdded={(e) => (e ? setError(e) : load())} />
      <p className="muted small">
        BYTE sends a notification ahead of time (bills 3 days, dates 2 weeks, maintenance a week). {tab === "package" && "It links to the carrier's own tracking page; it doesn't read the status itself."}
      </p>
    </>
  );
}
