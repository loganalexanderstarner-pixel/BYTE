#!/usr/bin/env node
// Adds the "details" each model shows when opened in the model list:
// a short description from the original model's card on Hugging Face, who
// made it, how strong it is at different things (BYTE's estimate from size,
// family, tags and recency), ideas for using it, and for community models a
// plain note on what's different about them.
//
// Runs after build-catalog.mjs and edits src-tauri/catalog/models.json in
// place. The start of each card is cached in scripts/catalog-cards.json
// (git-ignored), so re-runs only fetch new models.
//
//   node scripts/build-catalog.mjs && node scripts/enrich-catalog.mjs
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const HF = "https://huggingface.co";
const CATALOG = join(root, "src-tauri/catalog/models.json");
const CACHE = join(root, "scripts/catalog-cards.json");

/** Friendly names for the organizations behind models. */
const MAKERS = {
  qwen: "Alibaba (Qwen team)", google: "Google", "meta-llama": "Meta", mistralai: "Mistral AI", microsoft: "Microsoft",
  "deepseek-ai": "DeepSeek", "ibm-granite": "IBM", openai: "OpenAI", nvidia: "NVIDIA", liquidai: "Liquid AI",
  allenai: "Allen Institute for AI", huggingfacetb: "Hugging Face", nousresearch: "Nous Research", "zai-org": "Zhipu AI (Z.ai)",
  thudm: "Zhipu AI (Tsinghua)", moonshotai: "Moonshot AI", minimaxai: "MiniMax", tencent: "Tencent", baidu: "Baidu",
  coherelabs: "Cohere", cohereforai: "Cohere", tiiuae: "Technology Innovation Institute", internlm: "Shanghai AI Lab",
  "lgai-exaone": "LG AI Research", openbmb: "OpenBMB", "bytedance-seed": "ByteDance Seed", "stepfun-ai": "StepFun",
  xiaomimimo: "Xiaomi", inclusionai: "Ant Group (inclusionAI)", "arcee-ai": "Arcee AI", upstage: "Upstage", ai21labs: "AI21 Labs",
  "01-ai": "01.AI", "swiss-ai": "Swiss AI Initiative", "servicenow-ai": "ServiceNow", bigcode: "BigCode",
  cognitivecomputations: "Cognitive Computations (Eric Hartford)", thedrummer: "TheDrummer", sao10k: "Sao10K",
  mlabonne: "Maxime Labonne", "huihui-ai": "huihui-ai", davidau: "DavidAU", neversleep: "NeverSleep", undi95: "Undi95",
  anthracite: "Anthracite", "anthracite-org": "Anthracite", gryphe: "Gryphe", "arliai": "ArliAI", "sicariussicariistuff": "Sicarius",
};
/** GGUF re-uploaders: never the model's maker. */
const QUANTIZERS = new Set(["unsloth", "bartowski", "mradermacher", "quantfactory", "maziyarpanahi", "lmstudio-community", "ggml-org", "second-state", "thebloke", "triangle104", "lewdiculous", "readyart"]);

async function get(url, as = "json") {
  for (let attempt = 1; attempt <= 4; attempt++) {
    try {
      const r = await fetch(url);
      if (r.status === 404) return null;
      if (r.ok) return as === "json" ? r.json() : r.text();
      if (r.status === 429) await new Promise((res) => setTimeout(res, 4000 * attempt));
    } catch {
      await new Promise((res) => setTimeout(res, 1000 * attempt));
    }
  }
  return null;
}

/** The original model behind a GGUF repo (from its card's base_model). */
function originOf(info, repo) {
  let base = info?.cardData?.base_model;
  if (Array.isArray(base)) base = base[0];
  if (typeof base === "string" && base.includes("/") && !QUANTIZERS.has(base.split("/")[0].toLowerCase())) return base;
  // "bartowski/TheDrummer_Cydonia-24B-GGUF" → "TheDrummer/Cydonia-24B"
  const [owner, name] = repo.split("/");
  if (name.includes("_") && QUANTIZERS.has(owner.toLowerCase())) {
    const [origin, rest] = name.split(/_(.+)/);
    return `${origin}/${rest.replace(/[-_]GGUF$/i, "")}`;
  }
  return QUANTIZERS.has(owner.toLowerCase()) ? null : repo.replace(/[-_]GGUF$/i, "");
}

const ENTITIES = { "&nbsp;": " ", "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'", "&rsquo;": "’", "&mdash;": "—", "&ndash;": "–" };

/** Paragraphs that aren't about what the model is (setup, links, the author's bio…). */
const NOT_ABOUT = /quantiz|gguf|llama\.cpp|imatrix|download|huggingface-cli|ollama|lm studio|license|citation|@misc|bibtex|<think>|special thanks|original model|model creator|discord|recommend (deploying|using the following)|best practices|please (raise|open) an issue|contact us|fp8|bf16|vllm|sglang|transformers>=|pip install|^(hi|hello|hey)\b|^i'?m |^i am |ko-fi|patreon|support me|buy me|join (our|my)|^note:|^update|^changelog|^news|^\d{4}[./-]\d{2}|code snippet|how to (use|run|load)|you can (also )?try|thank(s| you)|sponsor|following (code|example)|sorted by size|ikawrakow|repacking|iq-quants|q4_0|how big a model|artefact2|evaluation set|on the none dataset|following results|^this repo(sitory)? (contains|provides)|^(usage|example|quickstart)\b/i;

/** The first real paragraph of a model card, in plain text. */
export function aboutFrom(readme) {
  if (!readme) return "";
  let t = readme.replace(/^---[\s\S]*?\n---\s*/, ""); // front matter
  t = t.replace(/```[\s\S]*?```/g, "").replace(/<!--[\s\S]*?-->/g, "").replace(/<(table|details|div)[\s\S]*?<\/\1>/gi, "");
  t = t.replace(/&[a-z#0-9]+;/gi, (e) => ENTITIES[e.toLowerCase()] ?? " ");
  // Headings, list items, tables and quotes are their own paragraphs.
  t = t.replace(/^\s*(#{1,6} .*)$/gm, "\n\n$1\n\n").replace(/^\s*([-*+] |\d+\. |\||>)/gm, "\n\n$1");
  const paras = t
    .split(/\n\s*\n/)
    .map((p) =>
      p
        .replace(/<[^>]+>/g, " ")
        .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
        .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
        .replace(/https?:\/\/\S+/g, "")
        .replace(/[*_`]+/g, "")
        .replace(/\s+/g, " ")
        .trim(),
    )
    .filter((p) => {
      if (p.length < 80 || /^#|^\||^>|^[-*+] |^\d+\. /.test(p)) return false;
      if (NOT_ABOUT.test(p) || (p.match(/\|/g) ?? []).length > 2) return false;
      const letters = (p.match(/[a-z]/gi) ?? []).length;
      return letters / p.length > 0.7;
    });
  let about = paras.slice(0, 2).join(" ");
  if (about.length > 460) {
    const cut = about.slice(0, 460);
    const end = Math.max(cut.lastIndexOf(". "), cut.lastIndexOf("! "));
    about = end > 150 ? cut.slice(0, end + 1) : `${cut.replace(/\s+\S*$/, "")}…`;
  }
  return about;
}

const clamp = (n) => Math.max(1, Math.min(5, Math.round(n)));

/** BYTE's estimate, 1–5, from quality, tags, family and size. */
export function strengths(m) {
  const tags = new Set(m.tags ?? []);
  const base = m.quality >= 80 ? 5 : m.quality >= 66 ? 4 : m.quality >= 50 ? 3 : m.quality >= 35 ? 2 : 1;
  const fam = (m.family ?? "").toLowerCase();
  const writingFam = /gemma|llama|mistral|hermes|cohere|community/.test(fam) || tags.has("writing") || tags.has("stories");
  const multiFam = /qwen|gemma|cohere|glm|exaone|ernie|hunyuan|yi|apertus|mistral/.test(fam) || tags.has("multilingual");
  const active = m.activeB ?? m.paramsB ?? 7;
  const speed = active <= 2 ? 5 : active <= 5 ? 4 : active <= 10 ? 3 : active <= 32 ? 2 : 1;
  const coder = tags.has("coding");
  const reasoner = tags.has("reasoning") || m.thinking;
  return {
    chat: clamp(base + (coder ? -1 : 0)),
    writing: clamp(base + (writingFam ? 1 : 0) - (coder ? 1 : 0)),
    coding: clamp(base + (coder ? 1 : 0) - (tags.has("stories") ? 1 : 0)),
    reasoning: clamp(base + (reasoner ? 1 : 0) - (tags.has("stories") ? 1 : 0)),
    math: clamp(base + (reasoner ? 1 : 0) - (m.paramsB && m.paramsB < 3 ? 1 : 0) - (tags.has("stories") ? 1 : 0)),
    languages: clamp(base + (multiFam ? 1 : -1)),
    speed,
  };
}

/** Ideas for using the model, from what it's good at. */
export function ideas(m) {
  const tags = new Set(m.tags ?? []);
  const out = [];
  if (tags.has("coding")) out.push("Explain an error message and fix it", "Write or refactor a script", "Review a pull request diff");
  if (tags.has("stories")) out.push("Co-write a story or a scene", "Play a character in an interactive story", "Brainstorm plots and worlds");
  if (tags.has("uncensored")) out.push("Blunt answers where other models refuse (check facts yourself)");
  if (tags.has("reasoning") || m.thinking) out.push("Work through a math or logic problem step by step", "Plan a project or a trip");
  if (tags.has("multilingual")) out.push("Translate a message or a menu", "Practise a language");
  if (tags.has("writing") && !tags.has("stories")) out.push("Draft an email or a cover letter", "Rewrite a paragraph in a different tone");
  if (tags.has("moe")) out.push("Big-model answers at small-model speed");
  if (tags.has("small") || tags.has("fast")) out.push("Quick answers on a smaller or older Mac");
  if (m.tools) out.push("Web search and tools inside BYTE");
  if (out.length < 3) out.push("Everyday questions and explanations", "Summarize an article");
  return [...new Set(out)].slice(0, 5);
}

function caution(m) {
  const tags = new Set(m.tags ?? []);
  if (tags.has("uncensored")) return "Safety tuning removed: it refuses much less and can produce content other models won't. Double-check facts.";
  if (tags.has("stories")) return "Tuned for fiction and role-play, not for facts: use a regular model for research.";
  if (tags.has("community")) return "Made by the community, not the original model's maker: quality varies.";
  return null;
}

async function main() {
  const cat = JSON.parse(readFileSync(CATALOG, "utf8"));
  const cache = existsSync(CACHE) ? JSON.parse(readFileSync(CACHE, "utf8")) : {};
  const todo = cat.models.filter((m) => m.role === "chat" && cache[m.repo]?.card === undefined);
  console.log(`${cat.models.length} models, ${todo.length} cards to fetch`);
  let done = 0;
  const worker = async () => {
    while (todo.length) {
      const m = todo.shift();
      const info = await get(`${HF}/api/models/${m.repo}`);
      const origin = originOf(info, m.repo);
      const readme = origin ? await get(`${HF}/${origin}/raw/main/README.md`, "text") : null;
      // A quantizer's card is about the files, not the model: never use it.
      const uploader = m.repo.split("/")[0].toLowerCase();
      const fallback = aboutFrom(readme) || QUANTIZERS.has(uploader) ? null : await get(`${HF}/${m.repo}/raw/main/README.md`, "text");
      const owner = origin?.split("/")[0] ?? null;
      // Keep the start of the card (enough for the description) so the text can be re-derived offline.
      const card = (readme && aboutFrom(readme) ? readme : fallback ?? readme ?? "").slice(0, 12000);
      cache[m.repo] = { origin, author: owner ? (MAKERS[owner.toLowerCase()] ?? owner) : null, card };
      if (++done % 25 === 0) {
        writeFileSync(CACHE, `${JSON.stringify(cache, null, 1)}\n`);
        process.stdout.write(`${done} `);
      }
    }
  };
  await Promise.all(Array.from({ length: 6 }, worker));
  writeFileSync(CACHE, `${JSON.stringify(cache, null, 1)}\n`);

  for (const m of cat.models) {
    if (m.role !== "chat") continue;
    const card = cache[m.repo] ?? {};
    m.details = {
      about: aboutFrom(card.card ?? "") || m.tagline || "",
      author: card.author ?? null,
      sourceUrl: card.origin ? `${HF}/${card.origin}` : `${HF}/${m.repo}`,
      strengths: strengths(m),
      ideas: ideas(m),
      community: (m.tags ?? []).includes("community"),
      caution: caution(m),
    };
  }
  writeFileSync(CATALOG, `${JSON.stringify(cat)}\n`);
  const withAbout = cat.models.filter((m) => m.details?.about && m.details.about !== m.tagline).length;
  console.log(`\nenriched ${cat.models.length} models (${withAbout} with a card description) → ${CATALOG}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
