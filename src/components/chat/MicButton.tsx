import { Loader2, Mic, Square, X } from "lucide-react";
import { forwardRef, useCallback, useImperativeHandle, useRef, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { SilenceDetector } from "../../lib/handsfree";
import { startRecording, type Recording } from "../../lib/recorder";
import { clock, toBase64 } from "../../lib/wav";
import { VoiceModels } from "./VoiceModels";
import { osText } from "../../lib/platform";

export interface MicHandle {
  /** Starts recording (holding Space); false when voice isn't set up yet. `auto`: stops by itself after a pause
   * (hands-free), and says "nothing" through onNothing when no one speaks. */
  start(opts?: { auto?: boolean; waitMs?: number }): Promise<boolean>;
  /** Stops and transcribes. */
  stop(): void;
  recording(): boolean;
}

type Phase = { kind: "idle" } | { kind: "recording"; since: number } | { kind: "transcribing" } | { kind: "setup" };

/** 🎤 in the message box: click to talk, click again to stop (or hold Space). The text lands in the box, unsent. */
export const MicButton = forwardRef<MicHandle, { onText: (text: string, auto: boolean) => void; onNothing?: () => void }>(function MicButton({ onText, onNothing }, ref) {
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [error, setError] = useState<string | null>(null);
  const [lvl, setLvl] = useState(0);
  const [, tick] = useState(0);
  const rec = useRef<Recording | null>(null);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);
  const auto = useRef<SilenceDetector | null>(null);
  const stopRef = useRef<(() => void) | null>(null);
  const nothingRef = useRef(onNothing);
  nothingRef.current = onNothing;

  const start = useCallback(async (opts?: { auto?: boolean; waitMs?: number }): Promise<boolean> => {
    if (rec.current) return true;
    setError(null);
    const st = inTauri ? await api.voiceStatus().catch(() => null) : null;
    if (inTauri && !st?.ready) {
      setPhase({ kind: "setup" });
      return false;
    }
    auto.current = opts?.auto ? new SilenceDetector(Date.now(), 1200, opts.waitMs ?? 20_000) : null;
    try {
      // "Hey BYTE" listening pauses while the mic is in use here.
      if (inTauri) void api.wakePause(true).catch(() => undefined);
      rec.current = await startRecording((l) => {
        setLvl(l);
        const heard = auto.current?.push(l, Date.now());
        if (heard === "done") stopRef.current?.();
        else if (heard === "nothing") {
          rec.current?.cancel();
          rec.current = null;
          auto.current = null;
          if (timer.current) clearInterval(timer.current);
          if (inTauri) void api.wakePause(false).catch(() => undefined);
          setPhase({ kind: "idle" });
          nothingRef.current?.();
        }
      });
    } catch (e) {
      if (inTauri) void api.wakePause(false).catch(() => undefined);
      setError(`BYTE can't use the microphone: ${errorText(e)}. Allow BYTE in System Settings → Privacy & Security → Microphone.`);
      return false;
    }
    setPhase({ kind: "recording", since: Date.now() });
    timer.current = setInterval(() => tick((n) => n + 1), 500);
    return true;
  }, []);

  const stop = useCallback(() => {
    const r = rec.current;
    const wasAuto = !!auto.current;
    rec.current = null;
    auto.current = null;
    if (timer.current) clearInterval(timer.current);
    if (inTauri) void api.wakePause(false).catch(() => undefined);
    if (!r) return;
    setPhase({ kind: "transcribing" });
    void r
      .stop()
      .then((wav) => (wav.length <= 44 + 16_000 / 2 ? "" : api.voiceTranscribe(toBase64(wav))))
      .then((text) => {
        if (text.trim()) onText(text.trim(), wasAuto);
        else if (wasAuto) nothingRef.current?.();
        else setError("BYTE didn't hear any speech.");
      })
      .catch((e) => setError(errorText(e)))
      .finally(() => setPhase({ kind: "idle" }));
  }, [onText]);

  stopRef.current = stop;

  const cancel = () => {
    rec.current?.cancel();
    rec.current = null;
    auto.current = null;
    if (inTauri) void api.wakePause(false).catch(() => undefined);
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
          <p className="muted small">{osText("A one-time download. Speech is turned into text on this Mac; your voice never leaves it.")}</p>
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
