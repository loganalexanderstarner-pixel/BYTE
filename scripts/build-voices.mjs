#!/usr/bin/env node
// Builds BYTE's voice catalog: src-tauri/catalog/voices.json.
//
// Every voice is free, made on the Mac by sherpa-onnx's speech tool (the
// `sherpa-tts` sidecar), and needs no account. Packages come from
// sherpa-onnx's `tts-models` release (one .tar.bz2 each). Each archive is
// streamed once to record its exact size and SHA-256 (cached in
// scripts/voices-cache.json, so re-runs only fetch new ones); the app checks
// both before unpacking.
//
// Providers:
//  - Kokoro (Apache-2.0): natural, high quality; 28 English voices.
//  - Piper (VITS, from the Rhasspy project): hundreds of voices in ~40
//    languages, small and quick; metadata from each voice's .onnx.json and
//    MODEL_CARD on Hugging Face (csukuangfj mirrors).
//  - Kitten TTS (Apache-2.0): tiny models with 8 expressive voices.
//  - Supertonic 3 (MIT code; OpenRAIL-M model): very fast, 10 voice styles, 31 languages.
//  - Pocket TTS (Kyutai; free for personal, non-commercial use): speaks in
//    the tone of a short reference recording.
//
//   node scripts/build-voices.mjs            # all
//   node scripts/build-voices.mjs --no-hash  # metadata only (keeps cached hashes)
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const RELEASE = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models";
const HF = "https://huggingface.co";
const CACHE = join(root, "scripts/voices-cache.json");
const OUT = join(root, "src-tauri/catalog/voices.json");
const NO_HASH = process.argv.includes("--no-hash");
const cache = existsSync(CACHE) ? JSON.parse(readFileSync(CACHE, "utf8")) : {};

async function get(url, as = "json") {
  for (let i = 0; i < 4; i++) {
    try {
      const r = await fetch(url, { redirect: "follow" });
      if (r.status === 404) return null;
      if (!r.ok) throw new Error(`${r.status} ${url}`);
      return as === "json" ? await r.json() : await r.text();
    } catch (e) {
      if (i === 3) throw e;
      await new Promise((ok) => setTimeout(ok, 1000 * 2 ** i));
    }
  }
}

/** Size and SHA-256 of a release archive (streamed, not stored). */
async function hashArchive(archive) {
  if (cache[archive]) return cache[archive];
  if (NO_HASH) return null;
  const r = await fetch(`${RELEASE}/${archive}`, { redirect: "follow" });
  if (!r.ok) {
    console.warn(`  skip ${archive}: ${r.status}`);
    return null;
  }
  const h = createHash("sha256");
  let size = 0;
  for await (const chunk of r.body) {
    h.update(chunk);
    size += chunk.length;
  }
  cache[archive] = { size, sha256: h.digest("hex") };
  writeFileSync(CACHE, JSON.stringify(cache, null, 1) + "\n");
  return cache[archive];
}

// ------------------------------------------------------------------ Kokoro
const KOKORO_EN = [
  ["af_heart", 3, "Heart", "en-US", "female", "Warm and friendly; the most natural-sounding Kokoro voice."],
  ["af_bella", 2, "Bella", "en-US", "female", "Bright and lively."],
  ["af_nicole", 6, "Nicole", "en-US", "female", "Soft, close and calm, almost a whisper."],
  ["af_sarah", 9, "Sarah", "en-US", "female", "Clear and even."],
  ["af_nova", 7, "Nova", "en-US", "female", "Crisp and modern."],
  ["af_sky", 10, "Sky", "en-US", "female", "Light and airy."],
  ["af_alloy", 0, "Alloy", "en-US", "female", "Neutral and steady."],
  ["af_aoede", 1, "Aoede", "en-US", "female", "Smooth and melodic."],
  ["af_jessica", 4, "Jessica", "en-US", "female", "Casual and upbeat."],
  ["af_kore", 5, "Kore", "en-US", "female", "Grounded and confident."],
  ["af_river", 8, "River", "en-US", "female", "Relaxed and easygoing."],
  ["am_michael", 16, "Michael", "en-US", "male", "Warm, friendly narrator."],
  ["am_fenrir", 14, "Fenrir", "en-US", "male", "Deep and strong."],
  ["am_puck", 18, "Puck", "en-US", "male", "Playful and quick."],
  ["am_echo", 12, "Echo", "en-US", "male", "Even and clear."],
  ["am_eric", 13, "Eric", "en-US", "male", "Confident and direct."],
  ["am_liam", 15, "Liam", "en-US", "male", "Young and casual."],
  ["am_onyx", 17, "Onyx", "en-US", "male", "Low and smooth."],
  ["am_adam", 11, "Adam", "en-US", "male", "Plain and steady."],
  ["am_santa", 19, "Santa", "en-US", "male", "Jolly and booming."],
  ["bf_emma", 21, "Emma", "en-GB", "female", "Polished British, warm."],
  ["bf_isabella", 22, "Isabella", "en-GB", "female", "Elegant British."],
  ["bf_alice", 20, "Alice", "en-GB", "female", "Gentle British."],
  ["bf_lily", 23, "Lily", "en-GB", "female", "Light, youthful British."],
  ["bm_george", 26, "George", "en-GB", "male", "Classic British narrator."],
  ["bm_fable", 25, "Fable", "en-GB", "male", "Storyteller, British."],
  ["bm_lewis", 27, "Lewis", "en-GB", "male", "Calm, measured British."],
  ["bm_daniel", 24, "Daniel", "en-GB", "male", "Clear, crisp British."],
].map(([id, sid, name, lang, gender, about]) => ({ id, sid, name, lang, gender, about }));

// ------------------------------------------------------------------ Kitten
const KITTEN = [
  ["expr-voice-2-m", "Jasper", "male"],
  ["expr-voice-2-f", "Bella", "female"],
  ["expr-voice-3-m", "Bruno", "male"],
  ["expr-voice-3-f", "Luna", "female"],
  ["expr-voice-4-m", "Hugo", "male"],
  ["expr-voice-4-f", "Rosie", "female"],
  ["expr-voice-5-m", "Leo", "male"],
  ["expr-voice-5-f", "Kiki", "female"],
].map(([id, name, gender], sid) => ({ id, sid, name, lang: "en-US", gender, about: "" }));

// ------------------------------------------------------------------ Supertonic (voice.bin is sorted F1…F5, M1…M5)
const SUPERTONIC = ["F1", "F2", "F3", "F4", "F5", "M1", "M2", "M3", "M4", "M5"].map((id, sid) => ({
  id,
  sid,
  name: `${id.startsWith("F") ? "Female" : "Male"} style ${id.slice(1)}`,
  lang: "en-US",
  gender: id.startsWith("F") ? "female" : "male",
  about: "",
}));
const SUPERTONIC_LANGS = "en ko ja ar bg cs da de el es et fi fr hi hr hu id it lt lv nl pl pt ro ru sk sl sv tr uk vi".split(" ");

// ------------------------------------------------------------------ Piper
// Speakers' genders for English single-speaker voices (Piper's metadata doesn't say).
const PIPER_GENDER = {
  amy: "female", lessac: "female", kathleen: "female", kristin: "female", ljspeech: "female", hfc_female: "female",
  jenny_dioco: "female", alba: "female", cori: "female", southern_english_female: "female", glados: "female",
  "sweetbbak-amy": "female", ryan: "male", joe: "male", kusal: "male", danny: "male", john: "male", norman: "male",
  bryce: "male", hfc_male: "male", alan: "male", northern_english_male: "male", southern_english_male: "male",
};
const PIPER_ABOUT = {
  lessac: "Expressive and clear; one of Piper's best-loved voices.",
  amy: "Friendly and natural.",
  ryan: "Steady, warm narrator.",
  joe: "Relaxed and casual.",
  kristin: "Bright and clear.",
  john: "Deep and calm.",
  norman: "Mature, measured narrator.",
  bryce: "Young and upbeat.",
  libritts_r: "Hundreds of different everyday voices to pick from (audiobook readers).",
  libritts: "Hundreds of different everyday voices to pick from (audiobook readers).",
  vctk: "109 speakers with many British and other English accents.",
  arctic: "18 speakers with different English accents.",
  l2arctic: "24 speakers who learned English as a second language.",
  semaine: "Four characters with distinct personalities (cheerful, gloomy, angry, sensible).",
  glados: "A robotic, sarcastic AI voice (for fun).",
  alba: "Scottish English.",
  northern_english_male: "Northern English accent.",
  jenny_dioco: "Irish-tinged English, warm.",
  cori: "Clear British English.",
};

function pretty(s) {
  return s.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

async function piperPackages() {
  const list = (await get(`${HF}/api/models?author=csukuangfj&search=vits-piper-&limit=1000`)) ?? [];
  const repos = list
    .map((m) => m.id)
    .filter((id) => id.startsWith("csukuangfj/vits-piper-") && !/-(int8|fp16)$/.test(id))
    .sort();
  const out = [];
  for (const repo of repos) {
    const name = repo.split("/")[1]; // vits-piper-en_US-amy-medium
    const stem = name.replace(/^vits-piper-/, ""); // en_US-amy-medium
    const meta = await get(`${HF}/${repo}/resolve/main/${stem}.onnx.json`).catch(() => null);
    if (!meta?.language) {
      console.warn(`  skip ${repo}: no metadata`);
      continue;
    }
    const card = (await get(`${HF}/${repo}/resolve/main/MODEL_CARD`, "text").catch(() => "")) ?? "";
    const parts = stem.split("-");
    const voice = parts.slice(1, -1).join("-");
    const quality = meta.audio?.quality ?? parts.at(-1);
    const lang = meta.language.code.replace("_", "-");
    const ids = Object.entries(meta.speaker_id_map ?? {}).sort((a, b) => a[1] - b[1]);
    const gender = lang.startsWith("en") ? (PIPER_GENDER[voice] ?? "") : "";
    const speakers = ids.length > 1
      ? ids.map(([sp, sid]) => ({ id: sp, sid, name: pretty(sp), lang, gender: /female/.test(sp) ? "female" : /male/.test(sp) ? "male" : "", about: "" }))
      : [{ id: voice, sid: 0, name: pretty(voice), lang, gender, about: "" }];
    const license = /License:\s*(.+)/i.exec(card)?.[1]?.trim();
    const archive = `${name}.tar.bz2`;
    const h = await hashArchive(archive);
    if (!h) continue;
    const country = meta.language.country_english ? ` (${meta.language.country_english})` : "";
    out.push({
      id: `piper-${stem}`,
      engine: "vits",
      provider: "Piper",
      name: `${pretty(voice)}${ids.length > 1 ? ` (${ids.length} voices)` : ""}`,
      about:
        PIPER_ABOUT[voice] ??
        `${meta.language.name_english}${country} voice from the Piper project${ids.length > 1 ? `, with ${ids.length} speakers` : ""}.`,
      archive,
      folder: name,
      size: h.size,
      sha256: h.sha256,
      ramMb: Math.round(h.size / 1e6 * 1.2 + 90),
      quality: quality === "x_low" ? "low" : quality,
      license: license && !/see url/i.test(license) ? license : "Open (see the voice's model card)",
      expressive: voice === "semaine" || voice === "lessac",
      languages: [lang],
      languageName: `${meta.language.name_english}${country}`,
      speakers,
    });
    process.stdout.write(".");
  }
  console.log(`\n${out.length} Piper packages`);
  return out;
}

async function fixed(p) {
  const h = await hashArchive(p.archive);
  return h ? { ...p, size: h.size, sha256: h.sha256 } : null;
}

const packages = [];
for (const p of [
  {
    id: "kokoro-v1_0",
    engine: "kokoro",
    provider: "Kokoro",
    name: "Kokoro",
    about: "Natural, warm and expressive: the closest to a big assistant's voice. 28 American and British voices.",
    archive: "kokoro-multi-lang-v1_0.tar.bz2",
    folder: "kokoro-multi-lang-v1_0",
    dir: "kokoro",
    ramMb: 520,
    quality: "high",
    license: "Apache-2.0",
    expressive: true,
    languages: ["en-US", "en-GB"],
    languageName: "English",
    speakers: KOKORO_EN,
  },
  {
    id: "kokoro-int8-v1_0",
    engine: "kokoro",
    provider: "Kokoro",
    name: "Kokoro (smaller)",
    about: "The same 28 voices in a smaller download that uses less memory; sounds nearly the same.",
    archive: "kokoro-int8-multi-lang-v1_0.tar.bz2",
    folder: "kokoro-int8-multi-lang-v1_0",
    ramMb: 330,
    quality: "high",
    license: "Apache-2.0",
    expressive: true,
    languages: ["en-US", "en-GB"],
    languageName: "English",
    speakers: KOKORO_EN,
  },
  {
    id: "kitten-mini-v0_8",
    engine: "kitten",
    provider: "Kitten TTS",
    name: "Kitten mini",
    about: "Eight expressive voices from a small, quick model. A good middle ground.",
    archive: "kitten-mini-en-v0_8.tar.bz2",
    folder: "kitten-mini-en-v0_8",
    ramMb: 330,
    quality: "medium",
    license: "Apache-2.0",
    expressive: true,
    languages: ["en-US"],
    languageName: "English",
    speakers: KITTEN,
  },
  {
    id: "kitten-nano-v0_8",
    engine: "kitten",
    provider: "Kitten TTS",
    name: "Kitten nano",
    about: "Tiny (about 30 MB) and light on memory: for older Macs or when a big model is loaded. Eight voices.",
    archive: "kitten-nano-en-v0_8-int8.tar.bz2",
    folder: "kitten-nano-en-v0_8-int8",
    ramMb: 140,
    quality: "medium",
    license: "Apache-2.0",
    expressive: true,
    languages: ["en-US"],
    languageName: "English",
    speakers: KITTEN,
  },
  {
    id: "supertonic-3",
    engine: "supertonic",
    provider: "Supertonic",
    name: "Supertonic 3",
    about: "Very fast, clear voices with ten speaking styles, in 31 languages (it speaks the language of the answer).",
    archive: "sherpa-onnx-supertonic-3-tts-int8-2026-05-11.tar.bz2",
    folder: "sherpa-onnx-supertonic-3-tts-int8-2026-05-11",
    ramMb: 280,
    quality: "high",
    license: "MIT (code), OpenRAIL-M (model)",
    expressive: false,
    languages: SUPERTONIC_LANGS,
    languageName: "31 languages",
    speakers: SUPERTONIC,
  },
  {
    id: "pocket-tts",
    engine: "pocket",
    provider: "Pocket TTS (Kyutai)",
    name: "Pocket TTS",
    about: "Speaks in the tone and feel of a short sample recording, so it sounds more like a real person. Slower to make.",
    archive: "sherpa-onnx-pocket-tts-int8-2026-01-26.tar.bz2",
    folder: "sherpa-onnx-pocket-tts-int8-2026-01-26",
    ramMb: 600,
    quality: "high",
    license: "CC-BY-4.0; free for personal, non-commercial use",
    expressive: true,
    languages: ["en-US"],
    languageName: "English",
    speakers: [
      { id: "bria", sid: 0, name: "Bria", lang: "en-US", gender: "female", ref: "test_wavs/bria.wav", about: "Warm, conversational." },
      { id: "loona", sid: 0, name: "Loona", lang: "en-US", gender: "female", ref: "test_wavs/loona.wav", about: "Bright and lively." },
    ],
  },
]) {
  const f = await fixed(p);
  if (f) packages.push(f);
}
packages.push(...(await piperPackages()));

writeFileSync(OUT, JSON.stringify({ version: 1, packages }, null, 1) + "\n");
const voices = packages.reduce((n, p) => n + p.speakers.length, 0);
console.log(`${packages.length} packages, ${voices} voices → ${OUT}`);
