import { api, inTauri } from "./api";
import type { SpeakerInfo } from "./types";

/** How many voices BYTE has been taught, remembered for a minute: a spoken question is only sent off to be recognised when there is someone to recognise. */
let cached: { at: number; n: number } | null = null;

export async function taughtCount(force = false): Promise<number> {
  if (!inTauri) return 0;
  if (!force && cached && Date.now() - cached.at < 60_000) return cached.n;
  const n = await api.voiceprintsList().then((l) => l.length, () => 0);
  cached = { at: Date.now(), n };
  return n;
}

export const forgetTaughtCount = () => {
  cached = null;
};

/** Whose voice is in this recording, when voices have been taught and BYTE Cloud is connected; `undefined` otherwise (nothing is sent). */
export async function whoIsSpeaking(wavBase64: string, cloudConnected: boolean): Promise<SpeakerInfo | undefined> {
  if (!cloudConnected || (await taughtCount()) === 0) return undefined;
  return api.voiceIdentify(wavBase64).catch(() => undefined);
}
