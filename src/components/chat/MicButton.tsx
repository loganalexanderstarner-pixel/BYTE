import { Loader2, Mic, Square, X } from "lucide-react";
import { forwardRef, useCallback, useImperativeHandle, useRef, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { startRecording, type Recording } from "../../lib/recorder";
import { clock, toBase64 } from "../../lib/wav";
import { VoiceModels } from "./VoiceModels";

export interface MicHandle {
  /** Starts recording (holding Space); false when voice isn't set up yet. */
  start(): Promise<boolean>;
  /** Stops and transcribes. */
  stop(): void;
  recording(): boolean;
}

type Phase = { kind: "idle" } | { kind: "recording"; since: number } | { kind: "transcribing" } | { kind: "setup" };

/** 🎤 in the message box: click to talk, click again to stop (or hold Space). The text lands in the box, unsent. */
export const MicButton = forwardRef<MicHandle, { onText: (text: string) => void }>(function MicButton({ onText }, ref) {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [error, setError] = useState<string | null>(null);
  const [lvl, setLvl] = useState(0);
  const [, tick] = useState(0);
  const rec = useRef<Recording | null>(null);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  const start = useCallback(async (): Promise<boolean> => {
    if (rec.current) return true;
    setError(null);
    const st = inTauri ? await api.voiceStatus().catch(() => null) : null;
    if (inTauri && !st?.ready) {
      setPhase({ kind: "setup" });
      return false;
    }
    try {
      rec.current = await startRecording(setLvl);
    } catch (e) {
      setError(`BYTE can't use the microphone: ${errorText(e)}. Allow BYTE in System Settings → Privacy & Security → Microphone.`);
      return false;
    }
    setPhase({ kind: "recording", since: Date.now() });
    timer.current = setInterval(() => tick((n) => n + 1), 500);
    return true;
  }, []);

  const stop = useCallback(() => {
    const r = rec.current;
    rec.current = null;
    if (timer.current) clearInterval(timer.current);
    if (!r) return;
    setPhase({ kind: "transcribing" });
    void r
      .stop()
      .then((wav) => (wav.length <= 44 + 16_000 / 2 ? "" : api.voiceTranscribe(toBase64(wav))))
      .then((text) => {
        if (text.trim()) onText(text.trim());
        else setError("BYTE didn't hear any speech.");
      })
      .catch((e) => setError(errorText(e)))
      .finally(() => setPhase({ kind: "idle" }));
  }, [onText]);

  const cancel = () => {
    rec.current?.cancel();
    rec.current = null;
    if (timer.current) clearInterval(timer.current);
    setPhase({ kind: "idle" });
  };

  useImperativeHandle(ref, () => ({ start, stop, recording: () => !!rec.current }), [start, stop]);

  return (
    <span className="mic">
      {phase.kind === "recording" ? (
        <>
          <button className="icon-btn mic-on" onClick={stop} title="Stop and type out what you said" aria-label="Stop recording">
            <Square size={13} />
          </button>
          <span className="mic-level" aria-hidden="true">
            <span style={{ transform: `scaleX(${Math.max(0.06, lvl)})` }} />
          </span>
          <span className="faint small mic-clock">{clock((Date.now() - phase.since) / 1000)}</span>
          <button className="icon-btn sm" onClick={cancel} title="Discard" aria-label="Discard recording">
            <X size={13} />
          </button>
        </>
      ) : phase.kind === "transcribing" ? (
        <span className="icon-btn" title="Typing out what you said…" aria-label="Transcribing">
          <Loader2 size={16} className="spin" />
        </span>
      ) : (
        <button className="icon-btn" onClick={() => void start()} title="Talk instead of typing (or hold Space in an empty box)" aria-label="Voice input">
          <Mic size={16} />
        </button>
      )}
      {phase.kind === "setup" && (
        <div className="mic-popover" role="dialog" aria-label="Set up voice input">
          <div className="row">
            <b className="grow">Voice input needs a speech model</b>
            <button className="icon-btn sm" onClick={() => setPhase({ kind: "idle" })} aria-label="Close">
              <X size={13} />
            </button>
          </div>
          <p className="muted small">A one-time download. Speech is turned into text on this Mac; your voice never leaves it.</p>
          <VoiceModels compact onReady={() => setPhase({ kind: "idle" })} />
        </div>
      )}
      {error && (
        <div className="mic-popover error" role="alert" onClick={() => setError(null)}>
          {error}
        </div>
      )}
    </span>
  );
});
