import { BatteryCharging, BatteryMedium, RotateCcw, SlidersHorizontal } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { api, errorText } from "../../lib/api";
import { bytes } from "../../lib/format";
import { displayName } from "../../lib/models";
import { BUDGET_OPTIONS, budgetLabel, isEmptyOverride, meterPercent, withOverride, withoutOverride } from "../../lib/tuning";
import type { LiveStats, ModelOverride } from "../../lib/types";
import { useStore } from "../../state/store";

const SAVE_DELAY_MS = 400;
const POLL_MS = 3000;

/** "Advanced tuning" in the Engine tab: per-model sampling overrides, live meters and battery saver. */
export function TuningPanel() {
  const engine = useStore((s) => s.engine);
  const models = useStore((s) => s.models);
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const key = engine.state === "ready" || engine.state === "starting" ? engine.model : null;

  const [draft, setDraft] = useState<ModelOverride>({});
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pending = useRef<{ key: string; draft: ModelOverride } | null>(null);

  const flush = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    const p = pending.current;
    pending.current = null;
    if (!p) return;
    const current = useStore.getState().settings?.modelOverrides;
    update({ modelOverrides: withOverride(withoutOverride(current, p.key), p.key, p.draft) }).catch((e) => setError(errorText(e)));
  };

  // Load the saved override when the loaded model changes (saving anything unsaved for the previous one first).
  useEffect(() => {
    flush();
    setDraft(key ? { ...(useStore.getState().settings?.modelOverrides?.[key] ?? {}) } : {});
  }, [key, !!settings]);

  // Save what's left when the panel closes.
  useEffect(() => () => flush(), []);

  const change = (patch: Partial<ModelOverride>) => {
    if (!key) return;
    setError(null);
    const next = { ...draft, ...patch };
    setDraft(next);
    pending.current = { key, draft: next };
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(flush, SAVE_DELAY_MS);
  };

  const resetAll = () => {
    if (!key) return;
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    pending.current = null;
    setDraft({});
    update({ modelOverrides: withoutOverride(useStore.getState().settings?.modelOverrides, key) }).catch((e) => setError(errorText(e)));
  };

  if (!settings) return null;

  return (
    <div className="section tuning">
      <h4>
        <SlidersHorizontal size={13} /> Advanced tuning
      </h4>
      {error && <div className="banner danger">{error}</div>}
      <LiveMeters />

      <label className="row battery-saver" style={{ gap: 10, cursor: "pointer" }}>
        <input type="checkbox" checked={settings.batterySaver ?? true} onChange={(e) => void update({ batterySaver: e.target.checked })} />
        <span>
          <b>Battery saver</b>
          <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
            Below 20% and unplugged, Deep and Extended answer like Auto, and thinking is kept short.
          </span>
        </span>
      </label>

      {!key ? (
        <p className="muted" style={{ margin: 0 }}>
          Load a model to tune it.
        </p>
      ) : (
        <>
          <div className="row tuning-head">
            <p className="muted grow" style={{ margin: 0 }}>
              For <b>{displayName(models, key, true)}</b>. BYTE uses each model's recommended settings; change them here only if you want
              to experiment. Changes apply to the next answer.
            </p>
            <button className="btn sm ghost" onClick={resetAll} disabled={isEmptyOverride(draft)}>
              <RotateCcw size={14} /> Reset all
            </button>
          </div>

          <SliderField
            id="tune-temp"
            label="Temperature"
            hint="Higher is more varied and creative; lower is more focused and predictable."
            min={0}
            max={2}
            step={0.05}
            fallback={0.7}
            value={draft.temperature ?? null}
            onChange={(v) => change({ temperature: v })}
          />
          <SliderField
            id="tune-topp"
            label="Top-p"
            hint="How many likely words the model picks from. Lower keeps it on the most likely ones."
            min={0.05}
            max={1}
            step={0.05}
            fallback={0.8}
            value={draft.topP ?? null}
            onChange={(v) => change({ topP: v })}
          />
          <div className="field">
            <label htmlFor="tune-budget">
              Thinking budget
              <small>How long the model may think before it answers.</small>
            </label>
            <select
              id="tune-budget"
              value={draft.thinkingBudget == null ? "" : String(draft.thinkingBudget)}
              onChange={(e) => change({ thinkingBudget: e.target.value === "" ? null : Number(e.target.value) })}
            >
              {BUDGET_OPTIONS.map((n) => (
                <option key={String(n)} value={n == null ? "" : String(n)}>
                  {budgetLabel(n)}
                </option>
              ))}
            </select>
          </div>
          <div className="tuning-extra">
            <label htmlFor="tune-extra">
              Extra instructions for this model
              <small>Added to every chat with this model, e.g. "Always answer in British English."</small>
            </label>
            <textarea
              id="tune-extra"
              className="text-input"
              rows={3}
              value={draft.systemExtra ?? ""}
              onChange={(e) => change({ systemExtra: e.target.value })}
              onBlur={flush}
            />
          </div>
        </>
      )}
    </div>
  );
}

function SliderField(props: {
  id: string;
  label: string;
  hint: string;
  min: number;
  max: number;
  step: number;
  /** Where the slider sits while the value is "Recommended". */
  fallback: number;
  value: number | null;
  onChange: (v: number | null) => void;
}) {
  const { id, label, hint, min, max, step, fallback, value, onChange } = props;
  return (
    <div className="field tuning-slider">
      <label htmlFor={id}>
        {label}
        <small>{hint}</small>
      </label>
      <div className="row tuning-control">
        <input
          id={id}
          type="range"
          min={min}
          max={max}
          step={step}
          value={value ?? fallback}
          className={value == null ? "is-default" : undefined}
          onChange={(e) => onChange(Math.round(Number(e.target.value) * 100) / 100)}
        />
        <output htmlFor={id} className={value == null ? "tuning-value faint" : "tuning-value"}>
          {value == null ? "Recommended" : value.toFixed(2)}
        </output>
        <button
          className="btn sm ghost icon-only"
          onClick={() => onChange(null)}
          disabled={value == null}
          title={`Recommended: go back to the model's recommended ${label.toLowerCase()}`}
          aria-label={`Recommended ${label.toLowerCase()}`}
        >
          <RotateCcw size={14} />
        </button>
      </div>
    </div>
  );
}

function LiveMeters() {
  const [live, setLive] = useState<LiveStats | null>(null);

  useEffect(() => {
    let stopped = false;
    let id: ReturnType<typeof setInterval> | null = null;
    const poll = () =>
      api
        .engineLive()
        .then((s) => !stopped && setLive(s))
        .catch(() => {
          if (id) clearInterval(id);
          id = null;
        });
    void poll();
    id = setInterval(poll, POLL_MS);
    return () => {
      stopped = true;
      if (id) clearInterval(id);
    };
  }, []);

  if (!live) return null;
  const ram = meterPercent(live.ramUsedBytes, live.ramTotalBytes);
  const engineShare = meterPercent(live.engineRssBytes, live.ramTotalBytes);
  const ramTone = ram >= 90 ? "danger" : ram >= 75 ? "warn" : "ok";
  const bat = live.battery;

  return (
    <div className="live-meters" aria-live="polite">
      <div className="meter">
        <div className="row meter-label">
          <span className="grow">Memory</span>
          <span className="faint">
            {bytes(live.ramUsedBytes)} of {bytes(live.ramTotalBytes)} used
          </span>
        </div>
        <div
          className={`meter-bar ${ramTone}`}
          role="meter"
          aria-label="Memory used"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={ram}
          style={{ ["--p" as string]: `${ram}%`, ["--e" as string]: `${Math.min(engineShare, ram)}%` }}
        >
          <i className="used" />
          <i className="engine" />
        </div>
      </div>
      <div className="meter-stats">
        <div>
          <span className="faint row" style={{ gap: 6 }}>
            <i className="meter-key" /> BYTE's engine
          </span>
          <b>{live.engineRssBytes != null ? bytes(live.engineRssBytes) : "Not running"}</b>
        </div>
        <div>
          <span className="faint">GPU can use</span>
          <b>{bytes(live.gpuBudgetBytes)}</b>
        </div>
        <div>
          <span className="faint">Battery</span>
          <b className="row" style={{ gap: 4 }}>
            {bat ? (
              <>
                {bat.charging ? <BatteryCharging size={14} /> : <BatteryMedium size={14} />} {Math.round(bat.percent)}%{bat.charging ? ", charging" : ""}
              </>
            ) : (
              "No battery"
            )}
          </b>
        </div>
      </div>
      {live.batterySaving && <div className="tuning-callout">Battery saver is on: lighter modes and shorter thinking until you plug in.</div>}
    </div>
  );
}
