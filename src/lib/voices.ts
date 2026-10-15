// BYTE's voice catalog in the UI: filtering, sorting and labels (voices.rs owns the data).
import type { VoicePackage, VoiceSpeaker } from "./types";

export const DEFAULT_VOICE = "kokoro-v1_0/af_heart";

export interface VoiceFilter {
  query: string;
  /** "" = any; else a language prefix like "en", "de", or "en-GB". */
  lang: string;
  gender: "" | "female" | "male";
  provider: string;
  expressive: boolean;
  /** Only voices that use little memory (≤ 200 MB while speaking). */
  light: boolean;
  /** Only downloaded ones. */
  downloaded: boolean;
}

export const NO_FILTER: VoiceFilter = { query: "", lang: "en", gender: "", provider: "", expressive: false, light: false, downloaded: false };

/** "kokoro-v1_0/af_heart" → its package and speaker (old settings named a Kokoro speaker alone). */
export function pick(catalog: VoicePackage[], setting: string | undefined): { pkg: VoicePackage; speaker: VoiceSpeaker } | null {
  const s = setting || DEFAULT_VOICE;
  const [pkgId, spId] = s.includes("/") ? s.split("/", 2) : ["kokoro-v1_0", s];
  const pkg = catalog.find((p) => p.id === pkgId) ?? catalog.find((p) => p.id === "kokoro-v1_0");
  if (!pkg) return null;
  return { pkg, speaker: pkg.speakers.find((x) => x.id === spId) ?? pkg.speakers[0] };
}

export const voiceSetting = (pkg: VoicePackage, speaker: VoiceSpeaker) => `${pkg.id}/${speaker.id}`;

const ORDER = ["Kokoro", "Kitten TTS", "Supertonic", "Pocket TTS (Kyutai)", "Piper"];
const QUALITY: Record<string, number> = { high: 3, medium: 2, low: 1 };

/** Matching packages, best first: providers in a sensible order, then quality, then name. */
export function filterVoices(catalog: VoicePackage[], f: VoiceFilter, ready: string[] = []): VoicePackage[] {
  const q = f.query.trim().toLowerCase();
  return catalog
    .filter((p) => !f.lang || p.languages.some((l) => l.toLowerCase().startsWith(f.lang.toLowerCase())))
    .filter((p) => !f.provider || p.provider === f.provider)
    .filter((p) => !f.expressive || p.expressive)
    .filter((p) => !f.light || p.ramMb <= 200)
    .filter((p) => !f.downloaded || ready.includes(p.id))
    .filter((p) => !f.gender || p.speakers.some((s) => s.gender === f.gender))
    .filter((p) => !q || [p.name, p.provider, p.about, p.languageName, ...p.speakers.slice(0, 50).map((s) => s.name)].join(" ").toLowerCase().includes(q))
    .sort((a, b) => {
      const pa = ORDER.indexOf(a.provider);
      const pb = ORDER.indexOf(b.provider);
      return pa - pb || (QUALITY[b.quality] ?? 0) - (QUALITY[a.quality] ?? 0) || a.name.localeCompare(b.name);
    });
}

/** The speakers to show for a package under the filter (gender), in catalog order. */
export function speakersFor(pkg: VoicePackage, f: Pick<VoiceFilter, "gender">): VoiceSpeaker[] {
  return pkg.speakers.filter((s) => !f.gender || s.gender === f.gender || !s.gender);
}

/** Languages in the catalog for the picker: [prefix, label, count], English first. */
export function languages(catalog: VoicePackage[]): [string, string, number][] {
  const m = new Map<string, [string, number]>();
  for (const p of catalog) {
    for (const l of p.languages) {
      const code = l.split("-")[0];
      const name = p.languages.length > 3 ? code : p.languageName.split(" (")[0];
      const cur = m.get(code);
      m.set(code, [cur && cur[0] !== code ? cur[0] : name, (cur?.[1] ?? 0) + 1]);
    }
  }
  return [...m.entries()]
    .map(([code, [name, n]]) => [code, name, n] as [string, string, number])
    .sort((a, b) => (a[0] === "en" ? -1 : b[0] === "en" ? 1 : a[1].localeCompare(b[1])));
}

export function providers(catalog: VoicePackage[]): string[] {
  return [...new Set(catalog.map((p) => p.provider))].sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b));
}

/** "about 170 MB of memory while speaking" in short form. */
export const ramLabel = (mb: number) => (mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${Math.round(mb / 10) * 10} MB`);

/** "American English · female" style line for a speaker. */
export function speakerLine(s: VoiceSpeaker): string {
  const accent: Record<string, string> = { "en-US": "American", "en-GB": "British" };
  return [accent[s.lang] ?? s.lang, s.gender].filter(Boolean).join(" · ");
}
