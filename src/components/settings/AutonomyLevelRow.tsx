import { Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { autoApi, LEVELS, type AutonomyLevel, type Rule } from "../../lib/auto";
import type { Settings } from "../../lib/types";
import { useStore } from "../../state/store";

/** Settings, Control: how much BYTE asks, and the "don't ask again" rules it has saved. */
export function AutonomyLevelRow({ settings }: { settings: Settings | null }) {
  const update = useStore((s) => s.updateSettings);
  const [rules, setRules] = useState<Rule[]>([]);
  const [error, setError] = useState<string | null>(null);
  const level: AutonomyLevel = settings?.controlNeverAsk ? "never" : (settings?.controlAutonomy ?? "ask");
  const current = LEVELS.find((l) => l.value === level) ?? LEVELS[0];

  const refresh = useCallback(() => {
    if (inTauri) autoApi.rules().then(setRules, (e) => setError(errorText(e)));
  }, []);
  useEffect(refresh, [refresh]);

  const choose = (v: AutonomyLevel) => {
    if (v === "never" && !window.confirm("Turn off every approval card for PC control? Program files, dangerous key combinations and payments are still refused, and Stop always works.")) return;
    void update({ controlAutonomy: v, controlNeverAsk: v === "never" });
  };
  const guard = async (f: () => Promise<unknown>) => {
    setError(null);
    try {
      await f();
      refresh();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <>
      <div className="field">
        <span>
          How much BYTE asks
          <small>{current.text}</small>
        </span>
        <select value={level} aria-label="How much BYTE asks" onChange={(e) => choose(e.target.value as AutonomyLevel)}>
          {LEVELS.map((l) => (
            <option key={l.value} value={l.value}>
              {l.label}
            </option>
          ))}
        </select>
      </div>
      <div className="field" style={{ display: "block" }}>
        <span>
          Things BYTE no longer asks about
          <small>These come from pressing “Approve and don't ask again” on a card. Each is for one kind of step, in one program, for you. Payments, program files, scripts and dangerous key combinations can never be added.</small>
        </span>
        {error && <small className="error">{error}</small>}
        {rules.length === 0 ? (
          <small>None yet.</small>
        ) : (
          <>
            <ul className="plain-list" aria-label="Saved rules" style={{ margin: "8px 0" }}>
              {rules.map((r) => (
                <li key={r.id} className="row" style={{ gap: 8 }}>
                  <span className="grow">
                    {r.text}
                    <small> · since {r.created}</small>
                  </span>
                  <button className="icon-btn" aria-label={`Remove: ${r.text}`} onClick={() => void guard(() => autoApi.ruleRemove(r.id))}>
                    <Trash2 size={14} />
                  </button>
                </li>
              ))}
            </ul>
            <button className="btn sm ghost" onClick={() => void guard(() => autoApi.rulesClear())}>
              <Trash2 size={14} /> Remove all
            </button>
          </>
        )}
      </div>
    </>
  );
}
