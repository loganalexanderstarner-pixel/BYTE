import { useEffect, useState } from "react";

import { api, inTauri } from "../../lib/api";
import { useStore } from "../../state/store";
import { currentOs, osText } from "../../lib/platform";

/** Settings → Voice: how "Hey BYTE" behaves around the phrase (indicator, chime, waiting, talking over BYTE, one voice, a talk key). */
export function VoiceControls() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  // The talk key is typed freely: it is applied on Enter or when the box loses focus, then checked (another program may own the key).
  const saved = settings?.talkHotkey ?? "";
  const [draft, setDraft] = useState(saved);
  const [keyError, setKeyError] = useState("");
  useEffect(() => {
    setDraft(saved);
  }, [saved]);
  const checkKey = () => (inTauri ? api.talkKeyStatus().then(setKeyError, () => setKeyError("")) : undefined);
  const applyKey = (v: string) => update({ talkHotkey: v.trim() }).then(checkKey);
  useEffect(() => {
    void checkKey();
  }, [settings?.wakeWord]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!settings?.wakeWord) return null;
  const suggested = "Ctrl+Alt+Space";
  return (
    <>
      <label className="field">
        <span>
          Show a small “listening” pill
          <small>A tiny dot at the top of the screen that shows when BYTE has heard “Hey BYTE” and is listening, thinking or speaking. It never takes focus and ignores clicks.</small>
        </span>
        <input type="checkbox" checked={settings.wakeIndicator ?? true} onChange={(e) => void update({ wakeIndicator: e.target.checked })} />
      </label>
      <label className="field">
        <span>
          Sound after “Hey BYTE”
          <small>{osText("The Mac's own sound, a soft two-note chime, or silence.")}</small>
        </span>
        <select value={settings.wakeChime ?? "system"} onChange={(e) => void update({ wakeChime: e.target.value as "system" | "soft" | "off" })} aria-label="Sound after Hey BYTE">
          <option value="system">System sound</option>
          <option value="soft">Soft chime</option>
          <option value="off">None</option>
        </select>
      </label>
      <label className="field">
        <span>
          Wait until I'm done
          <small>If a sentence clearly isn't finished (“and then…”, “um…”, ending on “the” or “to”), BYTE waits about one more second before answering and adds what you say next to the same question.</small>
        </span>
        <input type="checkbox" checked={settings.waitUntilDone ?? true} onChange={(e) => void update({ waitUntilDone: e.target.checked })} />
      </label>
      <label className="field">
        <span>
          Talking over BYTE
          <small>
            Headphones: any speech while BYTE talks lowers its volume, then stops it, and BYTE listens to what you said. Speakers: the same, but BYTE first subtracts its own voice from the microphone, and you need to speak up
            more clearly; this needs tuning in your room. Off: BYTE finishes first.
          </small>
        </span>
        <select value={settings.bargeIn ?? "off"} onChange={(e) => void update({ bargeIn: e.target.value as "off" | "headphones" | "speakers" })} aria-label="Talking over BYTE">
          <option value="headphones">Headphones (any speech)</option>
          <option value="speakers">Speakers (echo cancelled)</option>
          <option value="off">Off</option>
        </select>
      </label>
      <label className="field">
        <span>
          Focus on one voice
          <small>After “Hey BYTE”, only the voice that said it can talk over BYTE. Off (the default until it has been tuned on real voices): anyone. Strict: stricter matching. Matching uses simple voice features kept in memory only; it needs tuning with real voices. Say “listen to everyone”, “also listen to Anna” (a voice taught in Settings) or “stop focusing on me” to change it.</small>
        </span>
        <select value={settings.voiceFocus ?? "off"} onChange={(e) => void update({ voiceFocus: e.target.value as "off" | "normal" | "strict" })} aria-label="Focus on one voice">
          <option value="off">Off</option>
          <option value="normal">Normal</option>
          <option value="strict">Strict</option>
        </select>
      </label>
      <label className="field">
        <span>
          Talk key
          <small>
            A key that starts a listening turn without saying “Hey BYTE” (it works while “Hey BYTE” is on). Suggested: {suggested}. Leave it empty for no key. Keys need Ctrl, Alt or the Windows key
            {currentOs() === "mac" ? " (on a Mac: Control, Option or Command)" : ""} so normal typing still works.
          </small>
        </span>
        <span className="row">
          <input
            type="text"
            value={draft}
            placeholder="none"
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => draft.trim() !== saved && void applyKey(draft)}
            onKeyDown={(e) => e.key === "Enter" && (e.currentTarget.blur(), draft.trim() === saved && void checkKey())}
            aria-label="Talk key"
          />
          <button type="button" className="linklike" onClick={() => void applyKey(saved ? "" : suggested)}>
            {saved ? "Turn off" : `Use ${suggested}`}
          </button>
        </span>
        {keyError && <small role="alert">{keyError}</small>}
      </label>
    </>
  );
}
