import { useEffect, useState } from "react";

import { api, inTauri, type AudioDevices, type MicTestResult } from "../../lib/api";
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
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  useEffect(() => {
    if (inTauri) void api.audioDevices().then(setDevices, () => setDevices(null));
  }, []);
  if (!settings?.wakeWord) return null;
  const suggested = "Ctrl+Alt+Space";
  return (
    <>
      {devices && (devices.inputs.length > 0 || devices.outputs.length > 0) && (
        <>
          <label className="field">
            <span>
              Microphone
              <small>Which microphone BYTE listens with. “System default” follows whatever Windows or macOS uses, which is right for almost everyone.</small>
            </span>
            <select value={settings.micDevice ?? ""} onChange={(e) => void update({ micDevice: e.target.value })} aria-label="Microphone">
              <option value="">System default{devices.defaultInput ? ` (${devices.defaultInput})` : ""}</option>
              {devices.inputs.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>
              Speakers
              <small>Where BYTE’s voice plays. “System default” follows your system’s choice.</small>
            </span>
            <select value={settings.speakerDevice ?? ""} onChange={(e) => void update({ speakerDevice: e.target.value })} aria-label="Speakers">
              <option value="">System default{devices.defaultOutput ? ` (${devices.defaultOutput})` : ""}</option>
              {devices.outputs.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          <MicTest />
        </>
      )}
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
      {/* --- track:aec begin --- */}
      {currentOs() === "windows" && (settings.bargeIn ?? "off") === "speakers" && (
        <label className="field">
          <span>
            Use the sound card's own copy of what the speakers play
            <small>
              With speakers, BYTE removes its own voice (and any music) from the microphone using an exact copy of what Windows sends to the speakers, so you can talk over it and say “Hey BYTE” while it speaks. Off: BYTE
              uses its own playback only, which cannot see other programs and is less exact. The microphone is still the only thing recorded; nothing is saved.
            </small>
          </span>
          <input type="checkbox" checked={settings.aecLoopback ?? false} onChange={(e) => void update({ aecLoopback: e.target.checked })} />
        </label>
      )}
      {/* --- track:aec end --- */}
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

/** “Test my microphone”: five seconds of listening with a live level, then what BYTE heard and whether it would have woken. */
function MicTest() {
  const [phase, setPhase] = useState<"idle" | "listening" | "thinking">("idle");
  const [level, setLevel] = useState(0);
  const [result, setResult] = useState<MicTestResult | null>(null);
  useEffect(() => {
    let off: (() => void) | undefined;
    void api.onMicLevel((l) => setLevel(l)).then((u) => (off = u));
    return () => off?.();
  }, []);
  const run = () => {
    setResult(null);
    setPhase("listening");
    const timer = window.setTimeout(() => setPhase("thinking"), 5000);
    void api
      .micTest()
      .then(setResult)
      .catch(() => setResult({ peak: 0, heard: "", wake: false, opened: false }))
      .finally(() => (window.clearTimeout(timer), setPhase("idle"), setLevel(0)));
  };
  const verdict = !result
    ? ""
    : !result.opened
      ? "BYTE couldn’t open a microphone. Check that one is plugged in and that BYTE may use it (Windows Settings → Privacy → Microphone)."
      : result.peak < 0.02
        ? "BYTE heard nothing. Check the microphone above, its volume, and that it isn’t muted."
        : result.wake
          ? `Heard “${result.heard.trim()}”. That wakes BYTE.`
          : result.heard.trim()
            ? `Heard “${result.heard.trim()}”. That wouldn’t wake BYTE: say “Hey BYTE” a little clearer or closer.`
            : "BYTE heard sound but no words. Move closer or turn the microphone up.";
  return (
    <div className="field">
      <span>
        Test my microphone
        <small>Press the button and say “Hey BYTE”. BYTE listens for five seconds, then tells you what it heard. Nothing is kept.</small>
      </span>
      <span className="row">
        <button type="button" onClick={run} disabled={phase !== "idle"}>
          {phase === "listening" ? "Listening…" : phase === "thinking" ? "Working it out…" : "Test"}
        </button>
        <span className="mic-meter" aria-hidden="true">
          <span style={{ width: `${Math.round(level * 100)}%` }} />
        </span>
      </span>
      {verdict && <small role="status">{verdict}</small>}
    </div>
  );
}
