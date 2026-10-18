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
          After BYTE answers “Hey BYTE”
          <small>Like a smart speaker: BYTE answers, listens a few seconds for one follow-up, then stops until you say “Hey BYTE” again. Choose “Keep listening” to carry on a conversation until you say “thanks”, or “Stop right away” for no follow-up.</small>
        </span>
        <select value={settings.heyByteFollowUp ?? "once"} onChange={(e) => void update({ heyByteFollowUp: e.target.value as "off" | "once" | "chain" })}>
          <option value="once">One short follow-up</option>
          <option value="chain">Keep listening until I say thanks</option>
          <option value="off">Stop right away</option>
        </select>
      </label>
      <label className="field">
        <span>
          Test log (for troubleshooting)
          <small>While on, BYTE writes what “Hey BYTE” heard, whether it took it as the wake phrase and how long that took into its log, so a problem can be diagnosed. It records spoken words, so leave it off unless you are tracking a problem. “Prepare a report” in Backup and help includes it.</small>
        </span>
        <input type="checkbox" checked={settings.testLog ?? false} onChange={(e) => void update({ testLog: e.target.checked })} />
      </label>
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
      {/* --- track:wakemodel begin --- */}
      <label className="field">
        <span>
          Wake word detector
          <small>
            Quick: a tiny detector listens to every moment of sound, also while BYTE talks, and only when it thinks it heard “Hey BYTE” does the speech model confirm it. It saves processor time and reacts sooner. Careful: the speech
            model listens to every burst of speech, as before. It was trained on computer-made voices, so Careful is the safe choice if Quick misses you. A wake phrase of your own always uses Careful.
          </small>
        </span>
        <select value={settings.wakeDetector ?? "careful"} onChange={(e) => void update({ wakeDetector: e.target.value as "quick" | "careful" })} aria-label="Wake word detector">
          <option value="quick">Quick (new)</option>
          <option value="careful">Careful (speech model only)</option>
        </select>
      </label>
      {/* --- track:wakemodel end --- */}
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
