#!/usr/bin/env node
// Builds src-tauri/catalog/models.json from scripts/catalog-sources.json.
//
// For every listed model and quantization it asks Hugging Face for the exact
// file names, sizes and SHA-256 (supports multi-part "-00001-of-0000N" files),
// and reads the GGUF header of one file with an HTTP range request to learn
// the architecture numbers BYTE's RAM planner needs (layers, KV heads, head
// size, context length). Nothing large is downloaded.
//
//   node scripts/build-catalog.mjs && node scripts/enrich-catalog.mjs
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const sources = JSON.parse(readFileSync(join(root, "scripts/catalog-sources.json"), "utf8"));
const readList = (file) => {
  try {
    return JSON.parse(readFileSync(join(root, "scripts", file), "utf8")).models;
  } catch {
    return [];
  }
};
// Official models found by discover-models.mjs, then community fine-tunes (--community).
const discovered = [...readList("catalog-discovered.json"), ...readList("catalog-community.json")];
const HF = "https://huggingface.co";

async function json(url) {
  for (let attempt = 1; ; attempt++) {
    const r = await fetch(url);
    if (r.ok) return r.json();
    if (attempt >= 4) throw new Error(`${url}: HTTP ${r.status}`);
    await new Promise((res) => setTimeout(res, 1000 * attempt));
  }
}

const trees = new Map();
async function tree(repo) {
  if (!trees.has(repo)) trees.set(repo, await json(`${HF}/api/models/${repo}/tree/main?recursive=1`));
  return trees.get(repo);
}

/** All .gguf files in a repo (recursively), without projector/draft extras. */
async function listFiles(repo) {
  return (await tree(repo))
    .filter((f) => f.type === "file" && f.path.endsWith(".gguf") && !/mmproj|(^|\/)mtp[-/]/i.test(f.path))
    .map((f) => ({ name: f.path, size: f.lfs?.size ?? f.size, sha256: f.lfs?.oid ?? null }));
}

/**
 * A speed-up head shipped next to the model (multi-token prediction, EAGLE-3
 * or DSpark), used for speculative decoding instead of a separate small
 * model. Picks one 8-bit copy (4-bit when the 8-bit one is over 2 GB); skips
 * 16-bit copies and "shared" variants.
 */
export function pickSpeedHead(files) {
  const heads = files
    .filter((f) => f.type === "file" && f.path.endsWith(".gguf") && f.lfs?.oid)
    .map((f) => ({ name: f.path, size: f.lfs.size ?? f.size, sha256: f.lfs.oid, base: f.path.split("/").pop() }))
    .map((f) => ({ ...f, kind: /^(mtp|eagle3|dspark)[-_]/i.exec(f.base)?.[1]?.toLowerCase() }))
    .filter((f) => f.kind && !/BF16|F16|shared/i.test(f.base));
  if (!heads.length) return null;
  const rank = (f) => {
    const q = /(Q8_0|Q4_K_M|Q4_0)\.gguf$/i.exec(f.base)?.[1]?.toUpperCase();
    if (q === "Q8_0" || !q) return f.size > 2e9 ? 3 : 0;
    return q === "Q4_K_M" ? 1 : 2;
  };
  heads.sort((a, b) => rank(a) - rank(b) || a.name.length - b.name.length);
  const { kind, name, size, sha256 } = heads[0];
  return { kind, file: { name, size, sha256 } };
}

/**
 * Files for one quantization. Some repos hold the same version twice (one
 * file and a split copy in a folder); pick exactly one copy, preferring the
 * single file, and return split parts in order.
 */
function pickVariant(files, quant) {
  const esc = quant.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const re = new RegExp(`[-.]${esc}(-\\d{5}-of-\\d{5})?\\.gguf$`, "i");
  const matched = files.filter((f) => re.test(f.name.split("/").pop()) && !f.name.startsWith("BF16/"));
  const groups = new Map();
  for (const f of matched) {
    const key = f.name.replace(/-\d{5}-of-\d{5}(?=\.gguf$)/, "");
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(f);
  }
  const options = [...groups.values()].map((g) => g.sort((a, b) => a.name.localeCompare(b.name)));
  options.sort((a, b) => a.length - b.length || a[0].name.length - b[0].name.length);
  return options[0] ?? [];
}

// ---------- minimal GGUF metadata reader ----------

const T = { U8: 0, I8: 1, U16: 2, I16: 3, U32: 4, I32: 5, F32: 6, BOOL: 7, STRING: 8, ARRAY: 9, U64: 10, I64: 11, F64: 12 };
const SIZE = { 0: 1, 1: 1, 2: 2, 3: 2, 4: 4, 5: 4, 6: 4, 7: 1, 10: 8, 11: 8, 12: 8 };
class Short extends Error {}

function parseHeader(buf) {
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  let o = 0;
  const need = (n) => {
    if (o + n > buf.length) throw new Short();
  };
  const u32 = () => (need(4), (o += 4), dv.getUint32(o - 4, true));
  const u64 = () => (need(8), (o += 8), Number(dv.getBigUint64(o - 8, true)));
  const str = () => {
    const n = u64();
    need(n);
    const s = new TextDecoder().decode(buf.subarray(o, o + n));
    o += n;
    return s;
  };
  const scalar = (t) => {
    need(SIZE[t]);
    const at = o;
    o += SIZE[t];
    switch (t) {
      case T.U8: return dv.getUint8(at);
      case T.I8: return dv.getInt8(at);
      case T.U16: return dv.getUint16(at, true);
      case T.I16: return dv.getInt16(at, true);
      case T.U32: return dv.getUint32(at, true);
      case T.I32: return dv.getInt32(at, true);
      case T.F32: return dv.getFloat32(at, true);
      case T.BOOL: return dv.getUint8(at) !== 0;
      case T.U64: return Number(dv.getBigUint64(at, true));
      case T.I64: return Number(dv.getBigInt64(at, true));
      case T.F64: return dv.getFloat64(at, true);
    }
  };
  const value = (t, keep) => {
    if (t === T.STRING) return str();
    if (t === T.ARRAY) {
      const it = u32();
      const n = u64();
      if (!keep) {
        // Skip big arrays (tokenizer vocab) without materializing them.
        if (it === T.STRING) for (let i = 0; i < n; i++) str();
        else {
          need(n * SIZE[it]);
          o += n * SIZE[it];
        }
        return null;
      }
      return Array.from({ length: n }, () => value(it, true));
    }
    return scalar(t);
  };
  if (new TextDecoder().decode(buf.subarray(0, 4)) !== "GGUF") throw new Error("not a GGUF file");
  o = 4;
  u32(); // version
  u64(); // tensor count
  const kvCount = u64();
  const kv = {};
  for (let i = 0; i < kvCount; i++) {
    const key = str();
    const t = u32();
    const keep = !key.startsWith("tokenizer.");
    kv[key] = value(t, keep);
  }
  return kv;
}

async function readMetadata(repo, file) {
  const url = `${HF}/${repo}/resolve/main/${file}`;
  for (const mb of [4, 16, 64]) {
    const r = await fetch(url, { headers: { Range: `bytes=0-${mb * 1024 * 1024 - 1}` } });
    if (!r.ok && r.status !== 206) throw new Error(`${url}: HTTP ${r.status}`);
    const buf = new Uint8Array(await r.arrayBuffer());
    try {
      return parseHeader(buf);
    } catch (e) {
      if (!(e instanceof Short)) throw e;
    }
  }
  throw new Error(`${file}: metadata larger than 64 MB`);
}

/** The numbers the RAM planner uses, from GGUF metadata. */
function archFrom(kv) {
  const a = kv["general.architecture"];
  const g = (k) => kv[`${a}.${k}`];
  const nLayer = g("block_count");
  const heads = g("attention.head_count");
  const headsKv = g("attention.head_count_kv") ?? heads;
  const kvArr = Array.isArray(headsKv) ? headsKv : null;
  const nHeadKv = kvArr ? Math.max(...kvArr) : headsKv;
  const headCount = Array.isArray(heads) ? Math.max(...heads) : heads;
  // Pure state-space models (Mamba) have no attention and no KV cache.
  if (!headCount) {
    return { arch: a, nLayer, kvLayers: 0, nHeadKv: 0, headDim: 0, maxCtx: g("context_length") ?? 8192, experts: 0, expertsUsed: 0 };
  }
  const keyLen = g("attention.key_length") ?? Math.round(g("embedding_length") / headCount);
  const valLen = g("attention.value_length") ?? keyLen;
  // Layers that keep a KV cache: hybrid models (Qwen3.5+, LFM2) only cache
  // their full-attention layers.
  let kvLayers = nLayer;
  if (kvArr) kvLayers = kvArr.filter((x) => x > 0).length;
  const interval = g("full_attention_interval");
  if (typeof interval === "number" && interval > 1) kvLayers = Math.ceil(nLayer / interval);
  return {
    arch: a,
    nLayer,
    kvLayers,
    nHeadKv,
    headDim: Math.round((keyLen + valLen) / 2),
    maxCtx: g("context_length") ?? 8192,
    experts: g("expert_count") ?? 0,
    expertsUsed: g("expert_used_count") ?? 0,
  };
}

/** Active parameters per token (billions) for mixture-of-experts models. */
function activeParams(entry, arch, totalB) {
  if (entry.activeB) return entry.activeB;
  const m = /[-_ ]a(\d+(?:\.\d+)?)b\b/i.exec(entry.id + " " + entry.name);
  if (m) return Number(m[1]);
  if (arch.experts > 1 && arch.expertsUsed > 0 && totalB) {
    // Shared layers are a small share of MoE weights; the rest is split across experts.
    return Math.round(totalB * (0.05 + 0.95 * (arch.expertsUsed / arch.experts)) * 10) / 10;
  }
  return null;
}

/** Estimated capability when not hand-scored: effective size and recency. */
function autoQuality(totalB, activeB, released) {
  const effective = activeB ? Math.sqrt(totalB * activeB) * 1.3 : totalB;
  const year = Number(String(released ?? "2024").slice(0, 4));
  const recency = year >= 2026 ? 8 : year === 2025 ? 2 : year === 2024 ? -6 : -12;
  return Math.max(15, Math.min(90, Math.round(28 + 14 * Math.log(Math.max(0.3, effective)) + recency)));
}

/** Rough bits per weight from the quant name, for quality labels. */
function bitsOf(quant) {
  const q = quant.toUpperCase().replace(/^UD-/, "");
  const table = [
    [/^IQ1_S/, 1.6], [/^IQ1_M/, 1.8], [/^IQ2_XXS/, 2.1], [/^IQ2_XS/, 2.3], [/^IQ2_S/, 2.5], [/^IQ2_M/, 2.7],
    [/^Q2_K/, 2.9], [/^IQ3_XXS/, 3.1], [/^IQ3_S/, 3.4], [/^Q3_K_S/, 3.5], [/^Q3_K_M/, 3.9], [/^Q3_K_XL/, 4.0],
    [/^IQ4_XS/, 4.3], [/^IQ4_NL/, 4.5], [/^MXFP4/, 4.25], [/^Q4_0/, 4.5], [/^Q4_K_S/, 4.6], [/^Q4_K_M/, 4.8],
    [/^Q4_K_XL/, 5.0], [/^Q5_K_S/, 5.5], [/^Q5_K_M/, 5.7], [/^Q6_K/, 6.6], [/^Q8_0/, 8.5], [/^BF16|^F16/, 16],
  ];
  for (const [re, b] of table) if (re.test(q)) return b;
  return 4.5;
}

async function totalParams(repo) {
  try {
    const info = await json(`${HF}/api/models/${repo}?expand[]=gguf`);
    return info.gguf?.total ? Math.round((info.gguf.total / 1e9) * 10) / 10 : null;
  } catch {
    return null;
  }
}

async function build(entry, role) {
  const files = (await listFiles(entry.repo)).filter((f) => !/^(dspark|draft|eagle|aux)[-_]/i.test(f.name.split("/").pop()));
  const paramsKnown = entry.paramsB ?? (await totalParams(entry.repo));
  const variants = [];
  for (const quant of entry.variants) {
    const parts = pickVariant(files, quant);
    if (!parts.length) {
      console.warn(`  ! ${entry.id}: no files for ${quant}`);
      continue;
    }
    if (parts.some((p) => !p.sha256)) throw new Error(`${entry.id} ${quant}: missing sha256`);
    // Sanity check: the file size must match parameters × bits per weight, so
    // an unrelated file with the right suffix is never listed as the model.
    const size = parts.reduce((s, p) => s + p.size, 0);
    if (paramsKnown) {
      const expected = (paramsKnown * 1e9 * bitsOf(quant)) / 8;
      // UD quants keep some layers at higher precision and small models have
      // large vocab tables, so allow generous headroom above the estimate.
      const upper = paramsKnown < 3 ? 2.8 : 2.0;
      if (size < expected * 0.55 || size > expected * upper) {
        console.warn(`  ! ${entry.id} ${quant}: ${(size / 1e9).toFixed(1)} GB doesn't match ${paramsKnown}B params — skipped`);
        continue;
      }
    }
    variants.push({
      quant,
      bits: bitsOf(quant),
      sizeBytes: parts.reduce((s, p) => s + p.size, 0),
      files: parts.map((p) => ({ name: p.name, size: p.size, sha256: p.sha256 })),
    });
  }
  // No Mac (max 512 GB) can load versions much larger than ~460 GB.
  for (let i = variants.length - 1; i >= 0; i--) if (variants[i].sizeBytes > 460e9) variants.splice(i, 1);
  if (!variants.length) throw new Error(`${entry.id}: no variants small enough for any Mac`);
  const smallest = variants[0];
  const meta = await readMetadata(entry.repo, smallest.files[0].name);
  const arch = archFrom(meta);
  const params = meta["general.size_label"] ?? null;
  const paramsB = paramsKnown;
  const activeB = activeParams(entry, arch, paramsB);
  if (activeB && !entry.tags?.includes("moe")) entry.tags = [...(entry.tags ?? []), "moe"];
  const override = sources.qualityOverrides?.[entry.id];
  if (typeof override === "number") entry.quality = override;
  else if (entry.auto) entry.quality = autoQuality(paramsB ?? 7, activeB, entry.released);
  const { variants: _v, auto: _a, ...rest } = entry;
  console.log(`  ✓ ${entry.id.slice(0, 34).padEnd(34)} ${arch.arch.padEnd(10)} kvLayers=${arch.kvLayers}/${arch.nLayer} ctx=${arch.maxCtx} ${variants.length} sizes`);
  const speedHead = role === "chat" ? pickSpeedHead(await tree(entry.repo)) : null;
  return { quality: 0, ...rest, role, sizeLabel: params, paramsB, activeB, arch, variants, ...(speedHead ? { speedHead } : {}) };
}

/** Runs `fn` over items with limited concurrency, keeping order. */
async function pool(items, n, fn) {
  const results = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: n }, async () => {
      while (next < items.length) {
        const i = next++;
        results[i] = await fn(items[i]);
      }
    }),
  );
  return results;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();

async function main() {
  const out = { version: 1, generated: new Date().toISOString().slice(0, 10), models: [] };
  for (const m of sources.models) out.models.push(await build(m, "chat"));
  for (const h of sources.helpers) out.models.push(await build(h, h.role));
  const ids = new Set(out.models.map((m) => m.id));
  const auto = await pool(discovered.filter((m) => !ids.has(m.id)), 6, async (m) => {
    try {
      return await build(m, "chat");
    } catch (e) {
      console.warn(`  ! skipped ${m.id}: ${e.message}`);
      return null;
    }
  });
  for (const m of auto) if (m && !ids.has(m.id)) (ids.add(m.id), out.models.push(m));

  const dest = join(root, "src-tauri/catalog/models.json");
  mkdirSync(dirname(dest), { recursive: true });
  writeFileSync(dest, `${JSON.stringify(out)}\n`);
  console.log(`wrote ${out.models.length} models to ${dest} (${(JSON.stringify(out).length / 1024).toFixed(1)} KB)`);
}
