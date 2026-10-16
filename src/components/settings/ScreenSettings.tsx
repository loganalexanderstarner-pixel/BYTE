import { useEffect, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import type { ScreenInfo, ScreenRecording } from "../../lib/types";
import { useStore } from "../../state/store";

const LENGTHS = [15, 30, 60, 120];
const RATES = [1, 2, 4];

/** The lines of the exclusion box as a clean list (trimmed, no blanks, no repeats). */
export function cleanList(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(/\r?\n|,/)) {
    const t = raw.trim();
    if (t && !seen.has(t.toLowerCase())) {
      seen.add(t.toLowerCase());
      out.push(t);
    }
  }
  return out;
}

/** One sentence for the replay's live state. */
export function recordingLine(s: ScreenRecording | undefined, frames: number): string {
  if (!s?.on) return "Not recording.";
  if (s.recording) return `Recording the last ${s.seconds} seconds (${frames} frames in memory).`;
  return `Paused${s.paused ? `: ${s.paused}` : ""}.`;
}

/** Settings → Features: everything that lets BYTE look at the screen. Off until switched on; nothing is ever saved or sent on its own. */
export function ScreenSettings() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [info, setInfo] = useState<ScreenInfo | null>(null);
  const [live, setLive] = useState<ScreenRecording | undefined>();
  const [error, setError] = useState<string | null>(null);

  const refresh = () => void api.screenStatus().then((i) => { setInfo(i); setLive(i.status); }, () => undefined);
  useEffect(() => {
    if (!inTauri) return;
    refresh();
    let off: (() => void) | undefined;
    void api.onScreenRecording((s) => { setLive(s); refresh(); }).then((u) => (off = u));
    return () => off?.();
  }, []);

  if (!inTauri || !info?.supported) return null;
  const seconds = settings?.replaySeconds ?? 30;
  const fps = settings?.replayFps ?? 1;
  const guard = (p: Promise<unknown>) => p.then(() => setError(null), (e) => setError(errorText(e)));

  return (
    <div className="screen-settings">
      <h4>Looking at your screen</h4>
      <p className="muted" style={{ margin: "0 0 8px" }}>
        All of this is off until you switch it on. BYTE only ever looks at the window in front, never at a password manager, a bank, a private
        browser window, a private chat or anything on your never-look-at list. A picture is kept in memory only for the last {seconds} seconds,
        never saved to disk, and never sent anywhere unless you ask a question and chose BYTE Cloud for it (then BYTE sends a description of the screen and the text read from it, not the picture; that text is kept in the chat on the cloud like any message, and is never saved to memory). Windows draws its own yellow border around a window while it is being captured.
      </p>
      {error && <p className="error">{error}</p>}

      <label className="field">
        <span>
          Ask about what's on my screen
          <small>“What do you think about this?”, “what's on my screen?”, “explain this error”: BYTE looks at the window in front once, reads its text, and answers.</small>
        </span>
        <input type="checkbox" aria-label="Ask about what's on my screen" checked={!!settings?.screenAsk} onChange={(e) => void guard(update({ screenAsk: e.target.checked }))} />
      </label>

      <label className="field">
        <span>
          Remember the last {seconds} seconds (replay)
          <small>
            Keeps a rolling clip of the window in front, in memory only, so you can ask “where did I go wrong?” or “how could I do better?” after a play.
            A vision model describes plays and mistakes from still frames; it works best for slower games and obvious mistakes, not frame-perfect tactics.
          </small>
        </span>
        <input type="checkbox" aria-label="Remember the last seconds of my screen" checked={!!settings?.replayBuffer} onChange={(e) => void guard(update({ replayBuffer: e.target.checked }))} />
      </label>

      {settings?.replayBuffer && (
        <div className="field">
          <span>
            Replay length and detail
            <small>{recordingLine(live, info.frames)} More frames a second costs a little more of the processor.</small>
          </span>
          <span className="row" style={{ gap: 8, flexWrap: "wrap", justifyContent: "flex-end" }}>
            <select aria-label="Replay length" value={seconds} onChange={(e) => void guard(update({ replaySeconds: Number(e.target.value) }))}>
              {[...new Set([...LENGTHS, seconds])].sort((a, b) => a - b).map((n) => (
                <option key={n} value={n}>{n} seconds</option>
              ))}
            </select>
            <select aria-label="Frames per second" value={fps} onChange={(e) => void guard(update({ replayFps: Number(e.target.value) }))}>
              {RATES.map((n) => (
                <option key={n} value={n}>{n} a second</option>
              ))}
            </select>
            <button className="btn sm" onClick={() => void api.screenClear().then(refresh)}>Forget it now</button>
          </span>
        </div>
      )}

      <label className="field">
        <span>
          Game advisor
          <small>“How's my city doing?”, “what's wrong here?”: BYTE looks at the game once, reads the numbers on screen and says what looks wrong. It says what it can't see, and never makes up a game's rules.</small>
        </span>
        <input type="checkbox" aria-label="Game advisor" checked={!!settings?.gameAdvisor} onChange={(e) => void guard(update({ gameAdvisor: e.target.checked }))} />
      </label>

      <label className="field">
        <span>
          Read it to me
          <small>“Read this page” or “read the selected text”: BYTE copies the selection (and puts your clipboard back) or reads the window's text, then says it aloud. Say “stop reading” to stop.</small>
        </span>
        <input type="checkbox" aria-label="Read it to me" checked={!!settings?.readIt} onChange={(e) => void guard(update({ readIt: e.target.checked }))} />
      </label>

      {/* --- track:found begin --- one list for the screen features and PC control: see "Never touch" above */}
      <p className="muted">The programs and windows BYTE never looks at are the same ones it never operates: edit them in Never touch (in Control, above).</p>
      {/* --- track:found end --- */}
    </div>
  );
}
