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

# Real-life looks, for the "Realism" filter. Family defaults say what the base model is good at; words in a
# model's name add to them. Only models tagged "realism" get any.
LOOKS = {
    "portrait": "Portraits and people",
    "street": "Street and everyday life",
    "landscape": "Landscapes and nature",
    "animals": "Animals and wildlife",
    "product": "Product photos",
    "food": "Food photos",
    "architecture": "Buildings and interiors",
    "cinematic": "Movie-still look",
    "film": "Film and vintage photo look",
}
FAMILY_LOOKS = {
    "Z-Image": ["portrait", "street", "product", "food", "landscape", "cinematic"],
    "Qwen-Image 2.1": ["portrait", "product", "food", "architecture", "landscape", "street"],
    "FLUX.1": ["portrait", "landscape", "animals", "product", "food", "cinematic"],
    "Chroma": ["portrait", "film", "cinematic"],
    "ERNIE-Image": ["product", "architecture"],
    "Stable Diffusion 3.5": ["portrait", "landscape", "architecture"],
    "Stable Diffusion XL": ["portrait", "landscape", "cinematic"],
    "Stable Diffusion 1.5": ["portrait"],
}
NAME_LOOKS = [
    (r"portrait|face|skin|people|human|girl|man\b", "portrait"),
    (r"real|photo|epic|juggernaut|cyber|vision|lifelike", "portrait"),
    (r"street|candid|snap|amateur|iphone|instagram", "street"),
    (r"landscape|nature|scenery|outdoor|travel", "landscape"),
    (r"animal|wildlife|pet|bird", "animals"),
    (r"product|commercial|advert|ecommerce", "product"),
    (r"food|kitchen|culinary", "food"),
    (r"arch|interior|building|house|room", "architecture"),
    (r"cinema|movie|film-still|anamorphic|cine", "cinematic"),
    (r"film|analog|kodak|vintage|retro|35mm|grain", "film"),
]

def looks_for(fam, repo, tags):
    if "realism" not in tags:
        return []
    out = list(FAMILY_LOOKS.get(fam, ["portrait"]))
    r = repo.lower()
    for pat, k in NAME_LOOKS:
        if re.search(pat, r) and k not in out:
            out.append(k)
    return out

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
        # The photo-capable families are all "realism" models; the filter then offers real-life looks.
        tags = set(sp.get("tags", [])) | ({"realism"} if fam in ("Z-Image", "Qwen-Image 2.1", "FLUX.1") else set())
        card = d.get("cardData") or {}
        lic = card.get("license") or "unknown"
        lic_name = {"apache-2.0": "Apache-2.0", "mit": "MIT", "openrail++": "OpenRAIL++", "creativeml-openrail-m": "CreativeML OpenRAIL-M", "other": "Custom licence (read it before selling pictures)", "unknown": "Not stated"}.get(lic, lic)
        commercial = lic in ("apache-2.0", "mit", "openrail++", "creativeml-openrail-m")
        mature = bool(MATURE.search(sp["repo"])) or sp.get("mature", False)
        name = sp.get("name") or sp["repo"].split("/")[-1].replace("-GGUF", "").replace("-gguf", "").replace("_", " ")
        models.append({
            "id": re.sub(r"[^a-z0-9]+", "-", sp["repo"].lower()).strip("-"),
            "name": name, "family": fam, "kind": "generate",
            "tags": sorted(tags),
            "looks": looks_for(fam, sp["repo"], tags),
            "tagline": sp["tagline"], "strengths": sp["strengths"],
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

# Upscalers (kind "upscale"): small ESRGAN-family files that stable-diffusion.cpp reads directly.
UPSCALERS = [
    # (file, name, tags, tagline, strengths, quality, watch out)
    ("4xNomosWebPhoto_esrgan.gguf", "4xNomosWebPhoto", ["photo", "faces"], "The best for photos of people.", "Keeps real pore and wrinkle texture, so skin does not turn plastic. Tested best on a portrait.", 92),
    ("RealESRGAN_x4plus.gguf", "Real-ESRGAN x4plus", ["photo", "fast"], "The classic all-round upscaler.", "Clean and safe on photos and art. Can smooth skin a little.", 78),
    ("4xUltrasharpV10.gguf", "4x UltraSharp", ["art", "text"], "Crisp edges for art, screenshots and text.", "Very sharp lines. On faces it can look smooth.", 76),
    ("4x_foolhardy_Remacri.gguf", "4x Remacri", ["photo", "art"], "Soft natural enlargement.", "Gentle results for photos and paintings. Smooth skin.", 76),
    ("4x_foolhardy_Remacri_ExtraSmoother.gguf", "4x Remacri ExtraSmoother", ["photo"], "Remacri with extra smoothing.", "Hides compression blocks in low-quality photos.", 70),
    ("4xLSDIRplusR.gguf", "4x LSDIR plus R", ["photo"], "Strong clean-up for damaged photos.", "Good at removing noise and blur; skin comes out smooth.", 72),
    ("4xLSDIRplusN.gguf", "4x LSDIR plus N", ["photo"], "Keeps fine grain and texture.", "Adds a sandy grain; good for textures, rough on faces.", 70),
    ("4xLSDIRplusC.gguf", "4x LSDIR plus C", ["photo"], "LSDIR tuned for compressed images.", "Repairs JPEG artifacts.", 70),
    ("4xNomos2_otf_esrgan.gguf", "4xNomos2 otf", ["photo", "art"], "Textured, grainy enlargement.", "Lots of texture; can look noisy on skin.", 68),
    ("4xNomos8kSC.gguf", "4xNomos8k SC", ["photo", "art"], "Detail for large, clean images.", "Made for high-resolution sources.", 74),
    ("4x_NickelbackFS_72000_G.gguf", "4x NickelbackFS", ["art"], "Painterly, detailed enlargement.", "Good for illustrations and game art.", 70),
    ("4x_Nickelback_70000G.gguf", "4x Nickelback", ["art"], "An earlier Nickelback version.", "Good for illustrations.", 68),
    ("2x-ESRGAN.gguf", "2x ESRGAN", ["photo", "fast"], "Gentle 2x enlargement.", "Smaller step than 4x, keeps things natural.", 72),
    ("2xPSNR.gguf", "2x PSNR", ["photo"], "Faithful 2x enlargement without invented detail.", "Best when you want accuracy over sharpness.", 66),
    ("2x_StarSample_V2.0.gguf", "2x StarSample", ["art"], "2x enlargement for stylised art.", "Made for cartoon and game art.", 66),
    ("RealESRGAN_x4plus_anime_6B.gguf", "Real-ESRGAN anime 6B", ["anime"], "The standard upscaler for anime.", "Clean lines and flat colours; small and fast.", 84),
    ("smbss_2x_RRDB_Animation.gguf", "2x Animation RRDB", ["anime"], "2x enlargement for animation frames.", "Made for cartoons and animation.", 70),
]

def build_upscalers():
    repo = "wbruna/upscalers-sdcpp-gguf"
    out = []
    for fn, name, tags, tagline, strengths, q in UPSCALERS:
        try:
            f = fileinfo(repo, fn, "model")
        except SystemExit as e:
            print("upscaler skipped", fn, e, file=sys.stderr); continue
        scale = 2 if fn.startswith("2x") else 4
        out.append({
            "id": "upscale-" + re.sub(r"[^a-z0-9]+", "-", fn.lower().replace(".gguf", "")).strip("-"),
            "name": name, "family": "Upscaler", "kind": "upscale", "tags": sorted(set(tags)), "looks": [],
            "tagline": tagline, "strengths": strengths + f" Enlarges {scale}x.",
            "license": "Check the licence of the original model", "commercialOk": False, "community": True, "mature": False,
            "quality": q, "steps": scale, "overheadGb": 0.5, "downloads": 0, "source": repo,
            "variants": [{"quant": f"{scale}x", "bits": 16, "qualityCost": 0, "sizeBytes": f["size"], "files": [f]}],
            "companions": [],
        })
    return out

if __name__ == "__main__":
    specs = json.load(open(sys.argv[1]))
    ms = build(specs) + build_upscalers()
    cat = {"version": 2, "generated": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "models": ms}
    json.dump(cat, open("src-tauri/catalog/images.json", "w"), indent=1)
    print(len(ms), "models", sum(len(m["variants"]) for m in ms), "versions")
