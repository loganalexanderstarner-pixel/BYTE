#!/usr/bin/env python3
"""Builds src-tauri/catalog/images.json from Hugging Face (Image Studio, Windows).

Every file's size and SHA-256 come from the repository's own listing, so downloads can be verified.
Companion files (text encoders, VAE) per model family follow the stable-diffusion.cpp docs.
Usage: scripts/build-image-catalog.py [pool.json]   (pool.json: {repo: {dl, likes}} from a search)
"""
import json, re, sys, urllib.request, time

HF = "https://huggingface.co/"
UA = {"User-Agent": "byte-catalog"}

def get(url, tries=3):
    for i in range(tries):
        try:
            return json.load(urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=40))
        except Exception as e:
            if i == tries - 1:
                raise
            time.sleep(2)

_tree = {}
def tree(repo):
    if repo not in _tree:
        d = get(f"{HF}api/models/{repo}?blobs=true")
        _tree[repo] = d
    return _tree[repo]

def fileinfo(repo, name, role):
    for s in tree(repo).get("siblings", []):
        if s["rfilename"] == name:
            sha = (s.get("lfs") or {}).get("sha256")
            if not sha:
                raise SystemExit(f"no sha for {repo}/{name}")
            return {"role": role, "name": name, "repo": repo, "size": s["size"], "sha256": sha, "url": f"{HF}{repo}/resolve/main/{name}"}
    raise SystemExit(f"missing {repo}/{name}")

# Companion files each family needs (stable-diffusion.cpp docs, checked 2026-10-19).
FLUX_AE = ("camenduru/FLUX.1-dev-ungated", "ae.safetensors", "vae")
CLIP_L = ("comfyanonymous/flux_text_encoders", "clip_l.safetensors", "clip_l")
T5 = ("comfyanonymous/flux_text_encoders", "t5xxl_fp8_e4m3fn.safetensors", "t5xxl")
CLIP_G = ("Comfy-Org/stable-diffusion-3.5-fp8", "text_encoders/clip_g.safetensors", "clip_g")
FAMILIES = {
    "FLUX.1": dict(companions=[CLIP_L, T5, FLUX_AE], overhead=2.5, base=88, steps=4),
    "Chroma": dict(companions=[T5, FLUX_AE], overhead=2.5, base=80, steps=20),
    "Z-Image": dict(companions=[FLUX_AE, ("unsloth/Qwen3-4B-Instruct-2507-GGUF", "Qwen3-4B-Instruct-2507-Q4_K_M.gguf", "llm")], overhead=2.5, base=88, steps=8),
    "Qwen-Image 2.1": dict(companions=[("Comfy-Org/Qwen-Image-2.1", "vae/qwen_image_2.1_vae_bf16.safetensors", "vae"), ("Qwen/Qwen3-VL-8B-Instruct-GGUF", "Qwen3VL-8B-Instruct-Q4_K_M.gguf", "llm")], overhead=3.0, base=94, steps=20),
    "ERNIE-Image": dict(companions=[("Comfy-Org/ERNIE-Image", "vae/flux2-vae.safetensors", "vae"), ("unsloth/Ministral-3-3B-Instruct-2512-GGUF", "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf", "llm")], overhead=2.5, base=86, steps=8),
    "Stable Diffusion 3.5": dict(companions=[CLIP_L, CLIP_G, T5], overhead=2.0, base=78, steps=28),
    "Stable Diffusion XL": dict(companions=[], overhead=2.0, base=76, steps=25),
    "Stable Diffusion 1.5": dict(companions=[], overhead=1.5, base=55, steps=20),
}

QUALITY_COST = {"q8": 0, "q6": 1, "q5": 2, "q4": 4, "q3": 8, "q2": 15, "iq4": 5, "iq3": 9, "iq2": 16, "f16": 0, "bf16": 0, "fp16": 0, "fp8": 1, "f32": 0}
BITS = {"q8": 8.5, "q6": 6.6, "q5": 5.5, "q4": 4.5, "q3": 3.5, "q2": 2.6, "iq4": 4.3, "iq3": 3.3, "iq2": 2.4, "f16": 16, "bf16": 16, "fp16": 16, "fp8": 8, "f32": 32}

def quant_of(fname):
    m = re.search(r"(?i)(?<![a-z0-9])(iq\d(?:_[a-z0-9]+)*|q\d(?:_[a-z0-9]+)*|bf16|f16|fp16|fp8(?:_[a-z0-9]+)*|f32)(?![a-z0-9])", fname)
    return m.group(1).upper() if m else None

def qkey(q):
    return re.match(r"(?i)(iq\d|q\d|bf16|f16|fp16|fp8|f32)", q).group(1).lower()

MATURE = re.compile(r"(?i)uncensor|nsfw|heretic|abliter|biglove|lewd|erotic|porn")
SKIP = re.compile(r"(?i)mmproj|text[-_]?enc|vae|lora|edit|inpaint|controlnet|encoder|clip|t5xxl|qwen3-4b|qwen3-vl|mistral")

def family_of(repo):
    r = repo.lower()
    if "qwen-image-2.1" in r or "qwen-image-2512" in r and False: return "Qwen-Image 2.1"
    if "z-image" in r: return "Z-Image"
    if "ernie-image" in r: return "ERNIE-Image"
    if "chroma" in r: return "Chroma"
    if "flux.1" in r or "flux1" in r: return "FLUX.1"
    if "stable-diffusion-3.5" in r or "sd3.5" in r: return "Stable Diffusion 3.5"
    return None

def variants_gguf(repo, fam):
    out = []
    for s in tree(repo).get("siblings", []):
        n = s["rfilename"]
        base = n.split("/")[-1]
        if not n.endswith(".gguf") or SKIP.search(base) or s["size"] < 3e8:
            continue
        q = quant_of(base)
        sha = (s.get("lfs") or {}).get("sha256")
        if not q or not sha:
            continue
        k = qkey(q)
        out.append({"quant": q, "bits": BITS.get(k, 5.0), "qualityCost": QUALITY_COST.get(k, 6), "sizeBytes": s["size"],
                    "file": {"role": "model", "name": n, "repo": repo, "size": s["size"], "sha256": sha, "url": f"{HF}{repo}/resolve/main/{n}"}})
    seen = {}
    for v in out:  # one per quant name
        seen.setdefault(v["quant"], v)
    return sorted(seen.values(), key=lambda v: v["sizeBytes"])

def build(specs):
    models, comp_cache = [], {}
    for sp in specs:
        fam = sp["family"]
        F = FAMILIES[fam]
        try:
            d = tree(sp["repo"])
        except Exception as e:
            print("skip", sp["repo"], e, file=sys.stderr); continue
        if sp.get("files"):  # single-file safetensors
            vs = []
            for fn in sp["files"]:
                s = next((x for x in d["siblings"] if x["rfilename"] == fn), None)
                if not s or not (s.get("lfs") or {}).get("sha256"): print("skip file", sp["repo"], fn, file=sys.stderr); continue
                q = quant_of(fn) or "FP16"
                k = qkey(q)
                vs.append({"quant": q, "bits": BITS.get(k, 16), "qualityCost": QUALITY_COST.get(k, 0), "sizeBytes": s["size"],
                           "file": {"role": "model", "name": fn, "repo": sp["repo"], "size": s["size"], "sha256": s["lfs"]["sha256"], "url": f"{HF}{sp['repo']}/resolve/main/{fn}"}})
        else:
            vs = variants_gguf(sp["repo"], fam)
        if not vs:
            print("no variants", sp["repo"], file=sys.stderr); continue
        key = (fam)
        if key not in comp_cache:
            try:
                comp_cache[key] = [fileinfo(r, n, role) for r, n, role in F["companions"]]
            except SystemExit as e:
                print("companions failed", fam, e, file=sys.stderr); comp_cache[key] = None
        if comp_cache[key] is None:
            continue
        card = d.get("cardData") or {}
        lic = card.get("license") or "unknown"
        lic_name = {"apache-2.0": "Apache-2.0", "mit": "MIT", "openrail++": "OpenRAIL++", "creativeml-openrail-m": "CreativeML OpenRAIL-M", "other": "Custom licence (read it before selling pictures)", "unknown": "Not stated"}.get(lic, lic)
        commercial = lic in ("apache-2.0", "mit", "openrail++", "creativeml-openrail-m")
        mature = bool(MATURE.search(sp["repo"])) or sp.get("mature", False)
        name = sp.get("name") or sp["repo"].split("/")[-1].replace("-GGUF", "").replace("-gguf", "").replace("_", " ")
        models.append({
            "id": re.sub(r"[^a-z0-9]+", "-", sp["repo"].lower()).strip("-"),
            "name": name, "family": fam, "kind": "generate",
            "tags": sorted(set(sp.get("tags", []))), "tagline": sp["tagline"], "strengths": sp["strengths"],
            "license": lic_name, "commercialOk": commercial, "community": sp.get("community", True), "mature": mature,
            "quality": sp.get("quality", F["base"] - 3), "steps": sp.get("steps", F["steps"]),
            "overheadGb": F["overhead"], "downloads": sp.get("dl", 0),
            "source": sp["repo"],
            "variants": [{"quant": v["quant"], "bits": v["bits"], "qualityCost": v["qualityCost"], "sizeBytes": v["sizeBytes"], "files": [v["file"]]} for v in vs],
            "companions": comp_cache[key],
        })
    # Same name from different authors: say whose it is.
    from collections import Counter
    n = Counter(m["name"] for m in models)
    for m in models:
        if n[m["name"]] > 1:
            m["name"] = f"{m['name']} ({m['source'].split('/')[0]})"
    return models

if __name__ == "__main__":
    specs = json.load(open(sys.argv[1]))
    ms = build(specs)
    cat = {"version": 2, "generated": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "models": ms}
    json.dump(cat, open("src-tauri/catalog/images.json", "w"), indent=1)
    print(len(ms), "models", sum(len(m["variants"]) for m in ms), "versions")
