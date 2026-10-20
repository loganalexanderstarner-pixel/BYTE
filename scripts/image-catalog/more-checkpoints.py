#!/usr/bin/env python3
"""Adds community Stable Diffusion checkpoints (one .safetensors file, everything inside) to specs.json.

Reads a pool of repositories from Hugging Face (search by base model), keeps the ones that hold ready-to-use
single-file checkpoints, and writes hand-checkable spec entries. Run build-image-catalog.py afterwards.
Usage: more-checkpoints.py [pool.json]   (pool.json: [[repo, downloads], ...])
"""
import json, re, sys, time, urllib.request

UA = {"User-Agent": "byte-catalog"}
def get(u):
    for i in range(3):
        try:
            return json.load(urllib.request.urlopen(urllib.request.Request(u, headers=UA), timeout=40))
        except Exception:
            if i == 2:
                return None
            time.sleep(2)

BAD = re.compile(r"(?i)vae|lora|controlnet|inpaint|ip-?adapter|refiner|embedding|textual|lcm-lora|openpose|depth|canny|clip|t5|encoder|unet|lightning|turbo|hyper|dmd|-ema|ema-|sft")
MATURE = re.compile(r"(?i)uncensor|nsfw|lewd|erotic|porn|furry|hentai|ero\b")
ANIME = re.compile(r"(?i)anime|animagine|illustrious|pony|noob|anything|counterfeit|manga|waifu|cartoon|toon|comic|furry")
REAL = re.compile(r"(?i)real|photo|juggernaut|epic|cyber|vision|lifelike|analog|raw|portrait|skin")

def main():
    pool = json.load(open(sys.argv[1] if len(sys.argv) > 1 else "/tmp/sd_pool.json"))
    specs = json.load(open("specs.json"))
    have = {s["repo"] for s in specs}
    add = []
    for repo, dl in pool:
        if repo in have or dl < 3000:
            continue
        d = get(f"https://huggingface.co/api/models/{repo}?blobs=true")
        if not d:
            continue
        tags = set(d.get("tags") or [])
        if "lora" in tags or any(t.startswith("diffusers:") and "ControlNet" in t for t in tags):
            continue
        xl = "stable-diffusion-xl" in tags or any("XL" in t for t in tags if t.startswith("diffusers:")) or "-xl" in repo.lower() or "xl-" in repo.lower() or "sdxl" in repo.lower()
        files = []
        for s in d.get("siblings", []):
            n = s["rfilename"]
            if "/" in n or not n.endswith(".safetensors") or BAD.search(n):
                continue
            if not (s.get("lfs") or {}).get("sha256"):
                continue
            gb = s["size"] / 1e9
            if xl and not (3.0 <= gb <= 7.5):
                continue
            if not xl and not (1.5 <= gb <= 5.0):
                continue
            files.append((s["size"], n))
        if not files or "stable-diffusion" not in " ".join(tags) + repo.lower() and not xl:
            continue
        files = [n for _, n in sorted(files, reverse=True)[:3]]
        fam = "Stable Diffusion XL" if xl else "Stable Diffusion 1.5"
        r = repo.lower()
        t = []
        if REAL.search(r): t += ["realism"]
        if ANIME.search(r): t += ["cartoon", "illustration"]
        if not t: t = ["illustration"]
        if not xl: t += ["lowvram", "fast"]
        who = repo.split("/")[0]
        name = repo.split("/")[1].replace("-", " ").replace("_", " ")
        add.append(dict(repo=repo, family=fam, name=name, tags=sorted(set(t)), dl=dl, files=files, community=True,
                        mature=bool(MATURE.search(repo) or "not-for-all-audiences" in tags),
                        tagline=f"A community {'SDXL' if xl else 'Stable Diffusion 1.5'} model by {who}.",
                        strengths=f"A community-made {'SDXL' if xl else 'Stable Diffusion 1.5'} checkpoint. Not tested by us: read its page on Hugging Face ({repo}) for what it is tuned for."))
        print("+", repo, fam, files[:1], file=sys.stderr)
    specs += add
    json.dump(specs, open("specs.json", "w"), indent=1)
    print(len(add), "added;", len(specs), "specs")

main()
