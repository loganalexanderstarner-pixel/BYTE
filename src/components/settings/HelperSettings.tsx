import { useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { helperKeys, helperRows, minutesLabel, ASK_WHERE, MINUTES } from "../../lib/helpers";
import { isPcOs, prettyKeys } from "../../lib/keys";
import { useStore } from "../../state/store";
import { KeyRecorder } from "./KeyboardSection";
import { FocusOptions, HomeAssistantOptions, RecipeOptions } from "./HelperExtras";

/** Settings → Control and screen → Everyday helpers: small things BYTE does anywhere on the computer. Every one is off until switched on. */
export function HelperSettings() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [error, setError] = useState<string | null>(null);
  if (!inTauri || !settings) return null;
  const pc = isPcOs();
  const guard = (p: Promise<unknown>) => p.then(() => setError(null), (e) => setError(errorText(e)));
  const ask = settings.helperAskWhere ?? "local";
  const keys = helperKeys(settings, pc);

  return (
    <div className="helper-settings">
      <h4>Everyday helpers</h4>
      <p className="muted" style={{ margin: "0 0 8px" }}>
        Small helpers that work in any app. Each one is off until you switch it on, and each is only for this computer and this person. Nothing is
        saved from your screen, and nothing is sent to BYTE Cloud unless you choose Cloud below. They never run in a private chat or in kids mode,
        and never look at a password manager, a bank, a private browser window or anything on your never-look-at list.
      </p>
      {error && <p className="error">{error}</p>}

      <label className="field">
        <span>
          Ask where
          <small>Where the helpers' questions are answered. Local is this computer. Auto uses this computer and only falls back to BYTE Cloud for text that did not come from your screen. Text from your screen goes to BYTE Cloud only when you choose Cloud.</small>
        </span>
        <select aria-label="Where the helpers ask" value={ask} onChange={(e) => void guard(update({ helperAskWhere: e.target.value as "local" | "cloud" | "auto" }))}>
          {ASK_WHERE.map((o) => (
            <option key={o.id} value={o.id}>{o.label}</option>
          ))}
        </select>
      </label>

      {helperRows(pc).map((r) => (
        <div key={r.key}>
          <label className="field">
            <span>
              {r.title}
              <small>{r.text}</small>
            </span>
            <input type="checkbox" aria-label={r.title} checked={!!settings[r.key]} onChange={(e) => void guard(update({ [r.key]: e.target.checked }))} />
          </label>
          {r.key === "helperFocus" && settings.helperFocus && <FocusOptions settings={settings} update={update} />}
          {r.key === "helperHa" && settings.helperHa && <HomeAssistantOptions settings={settings} update={update} />}
          {r.key === "helperRecipes" && settings.helperRecipes && <RecipeOptions />}
          {r.key === "helperMeeting" && settings.helperMeeting && (
            <div className="field">
              <span>Longest recording<small>BYTE stops by itself at this length.</small></span>
              <select aria-label="Longest meeting recording" value={settings.helperMeetingMinutes ?? 120} onChange={(e) => void guard(update({ helperMeetingMinutes: Number(e.target.value) }))}>
                {[...new Set([...MINUTES, settings.helperMeetingMinutes ?? 120])].map((n) => (
                  <option key={n} value={n}>{minutesLabel(n)}</option>
                ))}
              </select>
            </div>
          )}
          {r.keys && settings[r.key] && (
            <div className="field">
              <span>Shortcut<small>Now {prettyKeys(keys[r.keys])}</small></span>
              <KeyRecorder value={keys[r.keys]} label={r.title} onSave={(k) => update({ [r.keys!]: k })} />
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
