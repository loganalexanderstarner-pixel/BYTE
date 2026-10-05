import { useState } from "react";

import { errorText } from "../../lib/api";
import { keysFromEvent, keysProblem, platformKeys as K, prettyKeys } from "../../lib/keys";
import type { Settings } from "../../lib/types";
import { useStore } from "../../state/store";

/** Click, then press the keys: records a global shortcut (Rust checks it again). */
function KeyRecorder({ value, label, onSave, disabled }: { value: string; label: string; onSave: (keys: string) => Promise<void>; disabled?: boolean }) {
  const [rec, setRec] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onKeyDown = async (e: React.KeyboardEvent) => {
    if (!rec) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && !e.metaKey && !e.altKey && !e.ctrlKey) return setRec(false);
    const keys = keysFromEvent(e.nativeEvent);
    if (!keys) return;
    const problem = keysProblem(keys);
    if (problem) return setError(problem);
    setRec(false);
    setError(null);
    try {
      await onSave(keys);
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <span className="key-recorder">
      <button
        className={`btn sm ${rec ? "primary" : "ghost"} keys`}
        disabled={disabled}
        aria-label={`${label} shortcut: ${prettyKeys(value)}. Click to change.`}
        onClick={() => {
          setError(null);
          setRec((r) => !r);
        }}
        onKeyDown={(e) => void onKeyDown(e)}
        onBlur={() => setRec(false)}
      >
        {rec ? "Press the keys…" : prettyKeys(value)}
      </button>
      {error && <small className="bad">{error}</small>}
    </span>
  );
}

const IN_APP: [string, string][] = [
  [K("⌘K"), "Command palette: find any chat, setting, mode or theme"],
  [K("⌘N"), "New chat"],
  [K("⌘,"), "Settings"],
  [K("⌘\\"), "Show or hide the sidebar"],
  ["Esc", "Stop the answer (in Quick Ask: hide the window)"],
  [K("Enter / ⇧Enter"), "Send / new line"],
];

/** Settings → About: Quick Ask, the menu-bar icon, the global shortcuts and the in-app keys. */
export function KeyboardSection({ settings }: { settings: Settings | null }) {
  const update = useStore((s) => s.updateSettings);
  const quickOn = settings?.quickAsk !== false;
  const selOn = settings?.selectionHotkey !== false && settings?.macControl !== false;
  return (
    <div className="section">
      <h4>Keyboard and menu bar</h4>
      <div className="field">
        <span>
          <label style={{ cursor: "pointer" }}>
            <input type="checkbox" checked={quickOn} onChange={(e) => void update({ quickAsk: e.target.checked })} /> Quick Ask
          </label>
          <small>A small window over any app: press the shortcut, ask, and the answer appears right there. “Open in BYTE” continues the chat here.</small>
        </span>
        <KeyRecorder value={settings?.quickAskKeys ?? "Alt+Space"} label="Quick Ask" disabled={!quickOn} onSave={(k) => update({ quickAskKeys: k })} />
      </div>
      <div className="field">
        <span>
          Selected text hotkey
          <small>Select text in any app and press it: the writing studio opens with that text. Turned on or off with Mac control above.</small>
        </span>
        <KeyRecorder value={settings?.selectionKeys ?? "Alt+Super+KeyB"} label="Selected text" disabled={!selOn} onSave={(k) => update({ selectionKeys: k })} />
      </div>
      <label className="field">
        <span>
          BYTE in the menu bar
          <small>Click the icon for Quick Ask; its menu shows BYTE or quits it.</small>
        </span>
        <input type="checkbox" checked={settings?.menuBarIcon !== false} onChange={(e) => void update({ menuBarIcon: e.target.checked })} />
      </label>
      <table className="keys-table">
        <tbody>
          {IN_APP.map(([k, what]) => (
            <tr key={k}>
              <td>
                <kbd>{k}</kbd>
              </td>
              <td className="muted small">{what}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
