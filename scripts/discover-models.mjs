#!/usr/bin/env node
// Discovers chat models for the catalog from trusted GGUF publishers on
// Hugging Face and writes scripts/catalog-discovered.json. build-catalog.mjs
// then merges it with the hand-curated scripts/catalog-sources.json.
//
// Rules: ungated; architecture supported by the pinned engine
// (scripts/llama-archs.json); chat models only (no embeddings, speech, OCR,
// image, draft/MTP files); one entry per base model, preferring the
// best-known quantizer.
//
// Two lists:
//  - official models (catalog-discovered.json): only official model lines;
//  - community models (catalog-community.json, --community): popular
//    fine-tunes and merges people make (Dolphin, Hermes, story and role-play
//    tunes, uncensored versions). Repos Hugging Face marks
//    "not-for-all-audiences" and explicit names stay out. Each is labelled as
//    community in the app, with its creator and what makes it different.
//
//   node scripts/discover-models.mjs [--target 650]
//   node scripts/discover-models.mjs --community [--target 160]
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const HF = "https://huggingface.co";
const argTarget = process.argv.indexOf("--target");
const COMMUNITY = process.argv.includes("--community");
const TARGET = argTarget > 0 ? Number(process.argv[argTarget + 1]) : COMMUNITY ? 220 : 650;
// Community list balance: at most this many uncensored versions, one per base model and size.
const MAX_UNCENSORED = 45;

const SUPPORTED = new Set(JSON.parse(readFileSync(join(root, "scripts/llama-archs.json"), "utf8")).architectures);
const curated = JSON.parse(readFileSync(join(root, "scripts/catalog-sources.json"), "utf8"));
const curatedRepos = new Set([...curated.models, ...curated.helpers].map((m) => m.repo.toLowerCase()));

// Publisher preference (lower index wins for the same base model).
// Trusted publishers of GGUF files (they re-upload official models faithfully).
const AUTHORS = ["unsloth", "ggml-org", "bartowski", "lmstudio-community", "Qwen", "google", "microsoft", "mistralai", "ibm-granite", "LiquidAI", "NousResearch", "allenai", "nvidia", "HuggingFaceTB", "LGAI-EXAONE", "tiiuae", "internlm", "openbmb", "second-state", "mradermacher", "QuantFactory", "MaziyarPanahi"];
const PER_AUTHOR = { unsloth: 1000, bartowski: 3000, "ggml-org": 400, "lmstudio-community": 1500, "second-state": 600, mradermacher: 3000, QuantFactory: 1000, MaziyarPanahi: 800 };
// Where community fine-tunes are found (quantizers re-uploading them, and creators who publish GGUF themselves).
const COMMUNITY_AUTHORS = ["bartowski", "mradermacher", "QuantFactory", "MaziyarPanahi", "lmstudio-community", "TheDrummer", "NousResearch", "DavidAU", "Triangle104", "mlabonne", "huihui-ai"];
// Explicit content stays out of the community list too.
const ADULT = /nsfw|erotic|lewd|porn|sex|hentai|smut|horny|fetish|nsfl|explicit|18\+|adult|criminal/i;

// Re-uploads named "<origin>_<model>" (bartowski style) must come from the
// model's official publisher, so community fine-tunes stay out.
const OFFICIAL_ORIGINS = new Set(
  ["qwen", "google", "meta-llama", "mistralai", "microsoft", "deepseek-ai", "ibm-granite", "liquidai", "huggingfacetb", "allenai", "lgai-exaone", "nousresearch", "nvidia", "bytedance-seed", "baidu", "tencent", "minimaxai", "moonshotai", "zai-org", "thudm", "cohereforai", "coherelabs", "tiiuae", "internlm", "openai", "xiaomimimo", "stepfun-ai", "inclusionai", "arcee-ai", "openbmb", "swiss-ai", "servicenow-ai", "upstage", "ai21labs", "01-ai", "shanghai_ai_laboratory"].map((s) => s.toLowerCase()),
);

// Only official model lines (name must start with one of these), so community
// fine-tunes and merges of unknown quality stay out.
const OFFICIAL = /^(qwen\d|qwen-|qwq|gemma-|medgemma|translategemma|functiongemma|gpt-oss|llama-|meta-llama|mistral-|ministral|magistral|devstral|codestral|mathstral|phi-|deepseek-|granite-|lfm\d|smollm|olmo|olmoe|exaone|hermes-\d|nemotron|nvidia-nemotron|llama-\d.*nemotron|seed-|ernie|hunyuan|minimax|kimi|glm-|command-r|aya-|falcon|internlm|ornith|mimo|step-|ling-|trinity|laguna|yi-|solar|jamba|apertus|minicpm|arcee|afm-|apriel|codegemma|codellama|starcoder2|qwen2|llama3)/;

// Content and quality filters: no uncensored/adult/roleplay re-tunes, no
// non-chat models, no vision-only models (vision arrives later), no merges.
const EXCLUDE = /uncensor|abliterat|heretic|obliterat|derestrict|nsfw|erotic|deepsex|roleplay|\brp\b|-rp-|lewd|jailbreak|stheno|lunaris|cydonia|tavern|dolphin|venice|drummer|whiterabbit|novelist|embed|rerank|tts|asr|whisper|ocr|speech|audio|vision|vlm|-vl-|-vl$|vl-gguf|\d\.\dv[-_]|omni|image|diffusion|flux|sdxl|mtp|draft|eagle|reward|-prm|guard|classifier|test|tiny|random|debug|-base-|-base$|pretrain|merge|slerp|distilled|styletune|agi|ins-v\d|or-not|uncenc|lorablat|readyart|scotoma|joycaption|llava|gutenberg|megabeam|special-tokens|reap|ream|whittle|turbo-|swift|anko|fable|claude|opus|gpt-?[45]|sonnet|i1$|-i1-/i;
// Community fine-tunes that re-upload under official-looking names, and
// outdated generations that newer models in the catalog replace.
const FINETUNE = /summary|deepsync|doctor|mesh|iterative|storm|open-sft|smoltalk|openreviewer|[-_]sft|dpo|simpo|sppo|wpo|[-_]cpo|kto|orpo|ablated|lcot|airoboros|prism|0\.6x|11\.5b|3-120b|3some|nephilim|rpmax|arliai|formax|\bsex\b|-sex|ataraxy|enigma|esper|cobalt|fireplace|shiningvaliant|hawkish|sentient|argunaut|supernova|sauerkraut|ultra-instruct|patronus|lynx|smaug|turbcat|boundless|lumen|yggdrasil|twin|eirai|bllossom|geminified|replete|agent007|magpie|chimera|litert|khanacademy|wikihow|luxia|alpha|centauri|wizardlm|sharegpt|docchat|chatqa|taide|elite|gradient|prolong|ezo|flight|evol|reinstruct|trinity-2|genrm|rlbff|prover|research-reasoning|gsm8k|tool-use|-old$|colbert|bpe-fix|-aps|262k/i;
const EXCLUDE_AUTHORS = new Set(["TheDrummer", "cognitivecomputations", "Sao10K", "NeverSleep", "Undi95"]);

async function json(url) {
  for (let attempt = 1; ; attempt++) {
    const r = await fetch(url);
    if (r.ok) return r.json();
    if (attempt >= 5) throw new Error(`${url}: HTTP ${r.status}`);
    await new Promise((res) => setTimeout(res, 1500 * attempt));
  }
}

/** "unsloth/Qwen3.8-27B-Instruct-GGUF" → "qwen3.8-27b". */
function baseName(repo) {
  return repo
    .split("/")[1]
    .toLowerCase()
    .replace(/[-_.]?gguf([-_]ud)?$/, "")
    .replace(/[-_](text|128k|1m)$/, "")
    .replace(/[-_](instruct|chat|it|hf|imatrix|i1|q\d.*|2507|2506|2503|2501)$/g, "")
    .replace(/[-_](instruct|chat|it|hf)$/g, "")
    .replace(/^[a-z0-9-]+_(?=[a-z])/, "") // "qwen_qwen3.5-4b", "zai-org_glm-4.7-flash"
    .replace(/^(meta-|openai-|zai-org-|nex-agi-|thudm-|huggingfacetb-|nousresearch-|moonshotai-|tencent-|baai-|kwaipilot-|xyzailab-|ibm-|nvidia-)/, "");
}

function paramsB(total) {
  return total ? Math.round((total / 1e9) * 10) / 10 : null;
}

/** Active parameters for mixture-of-experts names like "35B-A3B" or "A22B". */
function activeFromName(name) {
  const m = /[-_]a(\d+(?:\.\d+)?)b\b/i.exec(name);
  return m ? Number(m[1]) : null;
}

const FAMILIES = [
  [/qwen.*coder/, "Qwen", "Coding specialist from Alibaba's Qwen team.", ["coding"]],
  [/qwq|qwen/, "Qwen", "Alibaba's Qwen models: strong reasoning, tools and many languages.", ["reasoning", "multilingual"]],
  [/gemma/, "Gemma", "Google's Gemma models: polished writing and broad knowledge.", ["writing", "multilingual"]],
  [/gpt-oss/, "OpenAI", "OpenAI's open-weight reasoning models with tool use.", ["reasoning"]],
  [/llama/, "Llama", "Meta's Llama models: well-rounded general assistants.", ["writing"]],
  [/codestral|devstral/, "Mistral", "Mistral's coding models.", ["coding"]],
  [/mistral|magistral|ministral/, "Mistral", "Mistral AI's efficient European models.", ["writing", "multilingual"]],
  [/phi/, "Phi", "Microsoft's Phi models: strong reasoning for their size.", ["reasoning"]],
  [/deepseek.*coder/, "DeepSeek", "DeepSeek's coding models.", ["coding"]],
  [/deepseek|r1/, "DeepSeek", "DeepSeek reasoning models that think step by step.", ["reasoning"]],
  [/granite/, "Granite", "IBM's Granite models, built for business tasks and tools.", ["writing"]],
  [/lfm/, "Liquid", "Liquid AI's fast hybrid models, great on small Macs.", ["fast"]],
  [/smollm/, "SmolLM", "Hugging Face's small, fast models.", ["fast"]],
  [/olmo/, "OLMo", "Allen AI's fully open models.", ["reasoning"]],
  [/exaone/, "EXAONE", "LG's EXAONE models, strong in English and Korean.", ["multilingual"]],
  [/hermes/, "Hermes", "Nous Research's Hermes: capable, steerable assistants.", ["writing"]],
  [/nemotron/, "Nemotron", "NVIDIA's Nemotron models tuned for reasoning and tools.", ["reasoning"]],
  [/seed/, "Seed", "ByteDance Seed models with long context.", ["reasoning"]],
  [/ernie/, "ERNIE", "Baidu's ERNIE models.", ["multilingual"]],
  [/hunyuan/, "Hunyuan", "Tencent's Hunyuan models.", ["multilingual"]],
  [/minimax/, "MiniMax", "MiniMax's large mixture-of-experts models.", ["reasoning"]],
  [/kimi/, "Kimi", "Moonshot's Kimi models.", ["reasoning"]],
  [/ornith/, "Ornith", "Built for coding agents and step-by-step reasoning", ["reasoning"]],
  [/command|aya|cohere/, "Cohere", "Cohere's Command and Aya models: retrieval and many languages.", ["multilingual"]],
  [/falcon/, "Falcon", "TII's Falcon models.", ["writing"]],
  [/internlm/, "InternLM", "Shanghai AI Lab's InternLM models.", ["reasoning"]],
  [/starcoder/, "StarCoder", "BigCode's StarCoder coding models.", ["coding"]],
  [/yi/, "Yi", "01.AI's Yi models, strong in English and Chinese.", ["multilingual"]],
  [/solar/, "Solar", "Upstage's Solar models: capable and efficient.", ["writing"]],
  [/jamba/, "Jamba", "AI21's Jamba hybrid models with long context.", ["writing"]],
  [/apertus/, "Apertus", "Swiss AI's fully open multilingual models.", ["multilingual"]],
  [/minicpm/, "MiniCPM", "OpenBMB's MiniCPM: small models that punch above their size.", ["fast"]],
  [/arcee|afm/, "Arcee", "Arcee AI's efficient general models.", ["writing"]],
  [/apriel/, "Apriel", "ServiceNow's Apriel reasoning models.", ["reasoning"]],
  [/glm/, "GLM", "Zhipu's GLM: strong reasoning and coding, fluent in English and Chinese", ["reasoning", "multilingual"]],
];

function family(name) {
  const words = name.replace(/[-_.]/g, " ");
  for (const [re, fam, blurb, tags] of FAMILIES) {
    const bounded = new RegExp(`(^|\\s)(${re.source})`);
    if (bounded.test(words) || bounded.test(name)) return { family: fam, blurb, tags };
  }
  return { family: null, blurb: "Open model for general chat.", tags: [] };
}

const USE = {
  coding: "programming help, debugging and explaining code",
  reasoning: "math, analysis, planning and research questions",
  writing: "emails, essays, summaries and creative writing",
  multilingual: "translation and chatting in many languages",
  fast: "quick answers on smaller or older Macs",
  small: "quick answers on smaller or older Macs",
};

function describe(name, meta, p, active) {
  const fam = family(name);
  const tags = new Set(fam.tags);
  if (/coder|code|devstral|codestral/.test(name)) tags.add("coding");
  if (/r1|reason|think|qwq|magistral|phi-4-reasoning/.test(name) || meta.thinking) tags.add("reasoning");
  if (p !== null && p <= 4.5) tags.add("small").add("fast");
  if (active !== null) tags.add("moe");
  if (active !== null && active <= 5) tags.add("fast");
  const goodAt = [...tags].filter((t) => USE[t]).map((t) => USE[t]);
  return {
    family: fam.family,
    tags: [...tags],
    tagline: fam.blurb,
    usedFor: goodAt.length ? goodAt.slice(0, 2).join("; ") : "everyday questions and conversation",
  };
}

/** Rough capability score when not hand-curated: size and recency. */
function autoQuality(p, active, created) {
  const effective = active ? Math.sqrt(p * active) * 1.3 : p;
  const year = Number((created ?? "2024").slice(0, 4));
  const recency = year >= 2026 ? 8 : year === 2025 ? 2 : year === 2024 ? -6 : -12;
  return Math.max(15, Math.min(92, Math.round(28 + 14 * Math.log(Math.max(0.3, effective)) + recency)));
}

// Prefer these quants per model, smallest to largest.
const WANT = ["IQ2_M", "Q2_K", "IQ3_XXS", "Q3_K_M", "IQ4_XS", "Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0", "MXFP4"];

function pickQuants(files) {
  const found = [];
  for (const q of WANT) {
    const re = new RegExp(`[-._](UD-)?${q}(-\\d{5}-of-\\d{5})?\\.gguf$`, "i");
    const hit = files.find((f) => re.test(f.split("/").pop()) && !/^BF16\//i.test(f));
    if (hit) {
      const m = new RegExp(`(UD-)?${q}`, "i").exec(hit.split("/").pop());
      found.push(m[0].toUpperCase().replace(/^UD-/, "UD-"));
    }
  }
  // Keep a spread: at most 6 versions.
  return [...new Set(found)].slice(-6);
}

async function main() {
  const seen = new Map(); // base name → candidate
  // Curated models count as already seen, so they aren't listed twice.
  for (const m of [...curated.models, ...curated.helpers]) seen.set(baseName(m.repo), { curated: true });
  for (const author of AUTHORS) {
    const limit = PER_AUTHOR[author] ?? 120;
    const url = `${HF}/api/models?author=${author}&filter=gguf&sort=downloads&direction=-1&limit=${limit}&expand[]=gguf&expand[]=gated&expand[]=downloads&expand[]=createdAt`;
    let list;
    try {
      list = await json(url);
    } catch (e) {
      console.warn(`! ${author}: ${e.message}`);
      continue;
    }
    for (const m of list) {
      const arch = m.gguf?.architecture;
      if (!arch || !SUPPORTED.has(arch) || m.gated) continue;
      if (EXCLUDE.test(m.id)) continue;
      // Original author is often embedded in re-uploads: "bartowski/TheDrummer_X-GGUF".
      const repoName = m.id.split("/")[1];
      const origin = repoName.includes("_") ? repoName.split("_")[0] : null;
      if (origin && (EXCLUDE_AUTHORS.has(origin) || !OFFICIAL_ORIGINS.has(origin.toLowerCase()))) continue;
      if (!OFFICIAL.test(baseName(m.id)) || FINETUNE.test(baseName(m.id))) continue;
      if ((m.createdAt ?? "2025") < "2023-09") continue;
      if (!family(baseName(m.id)).family) continue;
      if (curatedRepos.has(m.id.toLowerCase())) continue;
      if ((m.downloads ?? 0) < 400) continue;
      const base = baseName(m.id);
      if (seen.has(base)) continue;
      const tmpl = m.gguf?.chat_template ?? "";
      if (!tmpl) continue; // no chat template → not a chat model
      seen.set(base, {
        repo: m.id,
        base,
        arch,
        total: m.gguf.total ?? null,
        ctx: m.gguf.context_length ?? null,
        downloads: m.downloads ?? 0,
        created: m.createdAt ?? null,
        tools: /tools/.test(tmpl),
        thinking: /enable_thinking|<think>|reasoning/.test(tmpl),
      });
    }
    console.log(`${author}: ${seen.size} candidates so far`);
  }

  const ranked = [...seen.values()]
    .filter((c) => !c.curated && c.total && c.total >= 0.25e9)
    .sort((a, b) => b.downloads - a.downloads);
  const out = [];
  const names = new Set(curated.models.map((m) => m.name.toLowerCase()));
  for (const c of ranked) {
    if (out.length >= TARGET) break;
    let files;
    try {
      const tree = await json(`${HF}/api/models/${c.repo}/tree/main?recursive=1`);
      files = tree.filter((f) => f.type === "file" && f.path.endsWith(".gguf") && !/mmproj|mtp/i.test(f.path)).map((f) => f.path);
    } catch {
      continue;
    }
    const variants = pickQuants(files);
    if (!variants.length) continue;
    // Must fit some Mac: 1.1 bytes/param at ~4 bits ≈ the smallest listed version.
    const approxSmallest = (c.total * (variants[0].includes("2") ? 0.35 : 0.45));
    if (approxSmallest > 380e9) continue;
    const name = c.repo.split("/")[1].replace(/^[^_]+_/, "").replace(/[-_]GGUF$/i, "").replace(/[-_]/g, " ").replace(/\s+/g, " ").trim();
    if (names.has(name.toLowerCase())) continue;
    names.add(name.toLowerCase());
    const p = paramsB(c.total);
    const active = activeFromName(c.base);
    const d = describe(c.base, c, p, active);
    out.push({
      id: c.base.replace(/[^a-z0-9.]+/g, "-").replace(/^-|-$/g, ""),
      name,
      family: d.family,
      released: c.created ? c.created.slice(0, 7) : null,
      tagline: d.tagline,
      usedFor: d.usedFor,
      tags: d.tags,
      thinking: c.thinking,
      tools: c.tools,
      license: null,
      quality: autoQuality(p ?? 7, active, c.created),
      paramsB: p,
      activeB: active,
      repo: c.repo,
      variants,
      auto: true,
    });
    process.stdout.write(".");
  }
  console.log(`\nwrote ${out.length} discovered models`);
  writeFileSync(join(root, "scripts/catalog-discovered.json"), `${JSON.stringify({ $comment: "Generated by scripts/discover-models.mjs — do not edit by hand; curate in catalog-sources.json instead.", generated: new Date().toISOString().slice(0, 10), models: out }, null, 1)}\n`);
}

const NOT_CHAT = /llava|joycaption|ui-mate|ui-tars|embed|rerank|tts|asr|whisper|ocr|speech|audio|vision|vlm|-vl-|-vl$|vl-gguf|omni|image|diffusion|flux|sdxl|mtp|draft|eagle|reward|-prm|guard|classifier|test|random|debug|-base-|-base$|pretrain|i1$|-i1-|special-tokens/i;

/** What kind of community model this is, from its name. */
function communityKind(base) {
  if (/abliterat|uncensor|heretic|obliterat|derestrict|lorablat/.test(base)) return "uncensored";
  if (/dolphin/.test(base)) return "dolphin";
  if (/roleplay|\brp\b|-rp-|cydonia|rocinante|magnum|stheno|lunaris|mythomax|tavern|behemoth|skyfall|anubis|valkyrie|fallen|unslop|nemomix|mn-12b|celeste|eva-|chronos|story|writer|novel|creative|gutenberg|darkest|dark-/.test(base)) return "stories";
  if (/coder|code/.test(base)) return "coding";
  if (/r1|reason|think|math/.test(base)) return "reasoning";
  return "general";
}

const KIND = {
  uncensored: ["Uncensored version: its safety tuning was removed, so it refuses far less.", ["uncensored"]],
  dolphin: ["Dolphin: an uncensored, very steerable assistant from Cognitive Computations.", ["uncensored", "writing"]],
  stories: ["Made by the community for stories, characters and role-play.", ["writing", "stories"]],
  coding: ["Community fine-tune for programming.", ["coding"]],
  reasoning: ["Community fine-tune for step-by-step reasoning.", ["reasoning"]],
  general: ["Community fine-tune or merge of an open model.", ["writing"]],
};

async function community() {
  const officialIds = new Set(JSON.parse(readFileSync(join(root, "scripts/catalog-discovered.json"), "utf8")).models.map((m) => m.id));
  const seen = new Map();
  for (const m of [...curated.models, ...curated.helpers]) seen.set(baseName(m.repo), { skip: true });
  for (const author of COMMUNITY_AUTHORS) {
    const limit = PER_AUTHOR[author] ?? 600;
    const url = `${HF}/api/models?author=${author}&filter=gguf&sort=downloads&direction=-1&limit=${limit}&expand[]=gguf&expand[]=gated&expand[]=downloads&expand[]=createdAt&expand[]=tags`;
    let list;
    try {
      list = await json(url);
    } catch (e) {
      console.warn(`! ${author}: ${e.message}`);
      continue;
    }
    for (const m of list) {
      const arch = m.gguf?.architecture;
      if (!arch || !SUPPORTED.has(arch) || m.gated) continue;
      if (NOT_CHAT.test(m.id) || ADULT.test(m.id) || (m.tags ?? []).includes("not-for-all-audiences")) continue;
      if ((m.downloads ?? 0) < 1500 || (m.createdAt ?? "2025") < "2024-01") continue;
      const base = baseName(m.id);
      // Official models belong in the regular list.
      const official = OFFICIAL.test(base) && !FINETUNE.test(base) && !EXCLUDE.test(m.id);
      if (official || officialIds.has(base.replace(/[^a-z0-9.]+/g, "-")) || seen.has(base)) continue;
      const tmpl = m.gguf?.chat_template ?? "";
      if (!tmpl) continue;
      seen.set(base, {
        repo: m.id,
        base,
        arch,
        total: m.gguf.total ?? null,
        downloads: m.downloads ?? 0,
        created: m.createdAt ?? null,
        tools: /tools/.test(tmpl),
        thinking: /enable_thinking|<think>|reasoning/.test(tmpl),
      });
    }
    console.log(`${author}: ${seen.size} candidates so far`);
  }
  const ranked = [...seen.values()].filter((c) => !c.skip && c.total && c.total >= 0.5e9).sort((a, b) => b.downloads - a.downloads);
  const out = [];
  const names = new Set();
  const uncensoredSeen = new Set();
  let uncensored = 0;
  for (const c of ranked) {
    if (out.length >= TARGET) break;
    const kindNow = communityKind(c.base);
    if (kindNow === "uncensored") {
      // "qwen2.5-14b-instruct-abliterated-v2" and "…-heretic" are the same model for the list.
      const key = `${(family(c.base).family ?? c.arch)}-${Math.round((c.total ?? 0) / 1e9)}`;
      if (uncensored >= MAX_UNCENSORED || uncensoredSeen.has(key)) continue;
      uncensoredSeen.add(key);
      uncensored++;
    }
    let files;
    try {
      const tree = await json(`${HF}/api/models/${c.repo}/tree/main?recursive=1`);
      files = tree.filter((f) => f.type === "file" && f.path.endsWith(".gguf") && !/mmproj|mtp/i.test(f.path)).map((f) => f.path);
    } catch {
      continue;
    }
    const variants = pickQuants(files);
    if (!variants.length) continue;
    if (c.total * 0.45 > 380e9) continue;
    const name = c.repo.split("/")[1].replace(/^[^_]+_/, "").replace(/[-_]GGUF$/i, "").replace(/[-_]/g, " ").replace(/\s+/g, " ").trim();
    if (names.has(name.toLowerCase())) continue;
    names.add(name.toLowerCase());
    const p = paramsB(c.total);
    const active = activeFromName(c.base);
    const kind = communityKind(c.base);
    const [tagline, kindTags] = KIND[kind];
    const fam = family(c.base).family ?? family(c.arch).family ?? "Community";
    // Uncensored versions, Dolphin and story/role-play tunes are community-made;
    // the rest are independent models from smaller makers (listed as regular models).
    const isCommunity = ["uncensored", "dolphin", "stories"].includes(kind);
    const tags = new Set([...(isCommunity ? ["community"] : []), ...kindTags]);
    if (p !== null && p <= 4.5) tags.add("small").add("fast");
    if (active !== null) tags.add("moe");
    if (c.thinking) tags.add("reasoning");
    out.push({
      id: `${isCommunity ? "community-" : ""}${c.base.replace(/[^a-z0-9.]+/g, "-").replace(/^-|-$/g, "")}`,
      name,
      family: fam,
      released: c.created ? c.created.slice(0, 7) : null,
      tagline: isCommunity ? tagline : family(c.base).blurb !== "Open model for general chat." ? family(c.base).blurb : "Open model from an independent maker.",
      usedFor: kind === "stories" ? "stories, characters and role-play" : kind === "coding" ? "programming help" : "everyday chat with a different personality",
      tags: [...tags],
      thinking: c.thinking,
      tools: c.tools,
      license: null,
      // Unknown quality: never ranked above official models of the same size.
      quality: Math.max(15, autoQuality(p ?? 7, active, c.created) - 6),
      paramsB: p,
      activeB: active,
      repo: c.repo,
      variants,
      auto: true,
    });
    process.stdout.write(".");
  }
  console.log(`\nwrote ${out.length} community models`);
  writeFileSync(join(root, "scripts/catalog-community.json"), `${JSON.stringify({ $comment: "Generated by scripts/discover-models.mjs --community — do not edit by hand.", generated: new Date().toISOString().slice(0, 10), models: out }, null, 1)}\n`);
}

if (COMMUNITY) community();
else main();
