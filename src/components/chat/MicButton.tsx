import { Loader2, Mic, Square, X } from "lucide-react";
import { forwardRef, useCallback, useImperativeHandle, useRef, useState } from "react";

import { currentDevice } from "../../lib/device";
import { api, errorText, inTauri } from "../../lib/api";
import { SilenceDetector } from "../../lib/handsfree";
import { startRecording, type Recording } from "../../lib/recorder";
import type { SpeakerInfo } from "../../lib/types";
import { whoIsSpeaking } from "../../lib/voiceId";
import { clock, toBase64 } from "../../lib/wav";
import { useStore } from "../../state/store";
import { VoiceModels } from "./VoiceModels";
import { osText } from "../../lib/platform";

export interface MicHandle {
  /** Starts recording (holding Space); false when voice isn't set up yet. `auto`: stops by itself after a pause
   * (hands-free), and says "nothing" through onNothing when no one speaks. */
  start(opts?: { auto?: boolean; waitMs?: number }): Promise<boolean>;
  /** BYTE's own microphone stream is listening for the turn after "Hey BYTE" (wake.rs): show it. */
  listening(): void;
  /** That turn is over: its audio (base64 WAV), or null when nothing was said. */
  heard(wavBase64: string | null): void;
  /** Stops and transcribes. */
  stop(): void;
  recording(): boolean;
}

type Phase = { kind: "idle" } | { kind: "recording"; since: number } | { kind: "transcribing" } | { kind: "setup" };

/** 🎤 in the message box: click to talk, click again to stop (or hold Space). The text lands in the box, unsent. */
let voiceReady = false;

export const MicButton = forwardRef<MicHandle, { onText: (text: string, auto: boolean, speaker?: SpeakerInfo, mood?: string) => void; onNothing?: () => void }>(function MicButton({ onText, onNothing }, ref) {
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
    // Asked once while it's ready: each question to the engine is a round trip in front of the first word.
    const st = inTauri && !voiceReady ? await api.voiceStatus().catch(() => null) : { ready: true };
    if (st?.ready) voiceReady = true;
    if (inTauri && !st?.ready) {
      setPhase({ kind: "setup" });
      return false;
    }
    auto.current = opts?.auto ? new SilenceDetector(Date.now(), 1100, opts.waitMs ?? 20_000) : null;
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
      setError(`BYTE can't use the microphone: ${errorText(e)}. ${currentDevice() === "phone" ? "Allow BYTE in Android Settings → Apps → BYTE → Permissions → Microphone." : osText("Allow BYTE in System Settings → Privacy & Security → Microphone.")}`);
      return false;
    }
    setPhase({ kind: "recording", since: Date.now() });
    timer.current = setInterval(() => tick((n) => n + 1), 500);
    return true;
  }, []);

  /** A recording (base64 WAV) turned into words, and handed on. */
  const process = useCallback(
    (b64: string, wasAuto: boolean): Promise<void> => {
      setPhase({ kind: "transcribing" });
      let speaker: Promise<SpeakerInfo | undefined> = Promise.resolve(undefined);
      let mood: string | undefined;
      return Promise.resolve()
        .then(() => {
          if (!b64) return "";
          // Whose voice it is, worked out while the words are being worked out (nothing is sent unless voices have been taught).
          speaker = whoIsSpeaking(b64, !!useStore.getState().settings?.cloudConnected);
          // A spoken question ("Hey BYTE", Talk mode) is listened to on the cloud when the answer comes from there; dictating into the box stays on this PC.
          if (wasAuto && inTauri) {
            const st = useStore.getState();
            const isPrivate = !!st.conversations.find((c) => c.id === st.currentId)?.private;
            return api.voiceTranscribeSmart(b64, isPrivate).then((h) => ((mood = h.mood ?? undefined), h.text));
          }
          return api.voiceTranscribe(b64);
        })
        .then(async (text) => {
          if (text.trim()) onText(text.trim(), wasAuto, await speaker, mood);
          else if (wasAuto) nothingRef.current?.();
          else setError("BYTE didn't hear any speech.");
        })
        .catch((e) => setError(errorText(e)))
        .finally(() => setPhase({ kind: "idle" }));
    },
    [onText],
  );

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
      .then((wav) => (wav.length <= 44 + 16_000 / 2 ? "" : toBase64(wav)))
      .then((b64) => process(b64, wasAuto));
  }, [process]);

  stopRef.current = stop;

  // The Rust mic stream is listening for the turn after "Hey BYTE" (no recording of our own).
  const external = useRef(false);
  const listening = useCallback(() => {
    external.current = true;
    setPhase({ kind: "recording", since: Date.now() });
  }, []);
  const heard = useCallback(
    (b64: string | null) => {
      if (!external.current) return;
      external.current = false;
      if (!b64) {
        setPhase({ kind: "idle" });
        nothingRef.current?.();
        return;
      }
      void process(b64, true);
    },
    [process],
  );

  const cancel = () => {
    if (external.current) {
      external.current = false;
      void api.wakeListen(0).catch(() => undefined);
    }
    rec.current?.cancel();
    rec.current = null;
    auto.current = null;
    if (inTauri) void api.wakePause(false).catch(() => undefined);
    if (timer.current) clearInterval(timer.current);
    setPhase({ kind: "idle" });
  };

  useImperativeHandle(ref, () => ({ start, stop, listening, heard, recording: () => !!rec.current || external.current }), [start, stop, listening, heard]);

  return (
    <span className="mic">
      {phase.kind === "recording" ? (
        <>
          <button className="icon-btn mic-on" onClick={external.current ? cancel : stop} title="Stop and type out what you said" aria-label="Stop recording">
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
