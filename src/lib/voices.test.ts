import { describe, expect, it } from "vitest";

import type { VoicePackage } from "./types";
import { filterVoices, languages, NO_FILTER, pick, providers, ramLabel, speakersFor, voiceSetting } from "./voices";

const sp = (id: string, gender: "female" | "male" | "", lang = "en-US") => ({ id, sid: 0, name: id, lang, gender, about: "" });
const pkg = (id: string, provider: string, extra: Partial<VoicePackage> = {}): VoicePackage => ({
  id,
  engine: "vits",
  provider,
  name: id,
  about: "",
  archive: `${id}.tar.bz2`,
  folder: id,
  size: 1e7,
  sha256: "x".repeat(64),
  ramMb: 170,
  quality: "medium",
  license: "MIT",
  expressive: false,
  languages: ["en-US"],
  languageName: "English (United States)",
  speakers: [sp("a", "female")],
  ...extra,
});

const catalog = [
  pkg("piper-de", "Piper", { languages: ["de-DE"], languageName: "German (Germany)", speakers: [sp("thorsten", "", "de-DE")] }),
  pkg("piper-amy", "Piper", { quality: "high" }),
  pkg("kokoro-v1_0", "Kokoro", { engine: "kokoro", ramMb: 520, expressive: true, speakers: [sp("af_heart", "female"), sp("am_michael", "male"), sp("bm_george", "male", "en-GB")] }),
  pkg("kitten-nano-v0_8", "Kitten TTS", { ramMb: 140, expressive: true }),
];

describe("voice catalog in the UI", () => {
  it("reads settings, old and new", () => {
    expect(pick(catalog, "kokoro-v1_0/am_michael")?.speaker.id).toBe("am_michael");
    expect(pick(catalog, "bm_george")?.speaker.lang).toBe("en-GB");
    expect(pick(catalog, undefined)?.speaker.id).toBe("af_heart");
    expect(pick(catalog, "gone/nobody")?.pkg.id).toBe("kokoro-v1_0");
    expect(voiceSetting(catalog[1], catalog[1].speakers[0])).toBe("piper-amy/a");
  });

  it("filters and sorts best first", () => {
    expect(filterVoices(catalog, NO_FILTER).map((p) => p.id)).toEqual(["kokoro-v1_0", "kitten-nano-v0_8", "piper-amy"]);
    expect(filterVoices(catalog, { ...NO_FILTER, lang: "de" }).map((p) => p.id)).toEqual(["piper-de"]);
    expect(filterVoices(catalog, { ...NO_FILTER, light: true }).map((p) => p.id)).toEqual(["kitten-nano-v0_8", "piper-amy"]);
    expect(filterVoices(catalog, { ...NO_FILTER, expressive: true, gender: "male" }).map((p) => p.id)).toEqual(["kokoro-v1_0"]);
    expect(filterVoices(catalog, { ...NO_FILTER, query: "michael" }).map((p) => p.id)).toEqual(["kokoro-v1_0"]);
    expect(filterVoices(catalog, { ...NO_FILTER, downloaded: true }, ["piper-amy"]).map((p) => p.id)).toEqual(["piper-amy"]);
    expect(speakersFor(catalog[2], { gender: "male" }).map((s) => s.id)).toEqual(["am_michael", "bm_george"]);
  });

  it("lists languages and providers", () => {
    const l = languages(catalog);
    expect(l[0][0]).toBe("en");
    expect(l.find((x) => x[0] === "de")?.[1]).toBe("German");
    expect(providers(catalog)).toEqual(["Kokoro", "Kitten TTS", "Piper"]);
    expect(ramLabel(171)).toBe("170 MB");
    expect(ramLabel(1520)).toBe("1.5 GB");
  });
});
