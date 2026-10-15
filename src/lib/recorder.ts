import { concat, downsample, encodeWav, level } from "./wav";

/** A microphone recording in progress (Web Audio; macOS asks once for the mic). */
export interface Recording {
  /** Stops and returns a 16 kHz mono WAV. */
  stop(): Promise<Uint8Array>;
  /** Stops and throws the audio away. */
  cancel(): void;
}

export async function startRecording(onLevel: (l: number) => void): Promise<Recording> {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } });
  const ctx = new AudioContext();
  const source = ctx.createMediaStreamSource(stream);
  // ScriptProcessor is old but everywhere, and needs no extra module file.
  const node = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  node.onaudioprocess = (e) => {
    const data = new Float32Array(e.inputBuffer.getChannelData(0));
    chunks.push(data);
    onLevel(level(data));
  };
  source.connect(node);
  node.connect(ctx.destination);
  const close = () => {
    node.disconnect();
    source.disconnect();
    stream.getTracks().forEach((t) => t.stop());
    void ctx.close();
  };
  return {
    async stop() {
      close();
      return encodeWav(downsample(concat(chunks), ctx.sampleRate));
    },
    cancel: close,
  };
}
