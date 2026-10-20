"""Plain names and descriptions for community models whose repo names read as a label instead of a model.

Cards say what a model is good at for pictures (realism, detail, light), taken from the base model it builds
on. Community models are not tested by us, and the card says so. Used by build-image-catalog.py and runnable
on its own to rewrite catalog/images.json in place.
"""
import json, re, sys
from pathlib import Path

UNTESTED = " Community-made and not tested by us, so try it beside the original."


def author(e):
    return e["source"].split("/")[0]


def neutral(e):
    """Returns (name, tagline, strengths, extra_tags) for a mature-marked entry, or None to leave it alone."""
    fam, src, nm = e["family"], e["source"].lower(), e["name"].lower()
    a = author(e)
    turbo = "turbo" in nm or "turbo" in src
    if fam == "Qwen-Image 2.1":
        name = f"Qwen-Image 2.1{' Turbo' if turbo else ''} · {a}"
        tag = f"Qwen-Image 2.1{' Turbo' if turbo else ''} variant by {a}."
        st = ("Photographic detail from the Qwen-Image 2.1 base: natural skin, believable light and readable text in the picture."
              + (" Makes a picture in about 4 steps." if turbo else "") + UNTESTED)
        return name, tag, st, ["realism"]
    if fam == "FLUX.1":
        return f"FLUX.1 dev · {a}", f"FLUX.1 dev variant by {a}.", "Detailed photographic realism from the FLUX.1 dev base: fine texture, natural light and good hands." + UNTESTED, ["realism"]
    if fam == "Z-Image":
        return f"Z-Image Turbo v2 · {a}", f"Z-Image Turbo variant by {a}.", "Fast, photographic pictures from the Z-Image Turbo base: sharp detail and natural light in a few steps." + UNTESTED, ["realism", "fast"]
    if fam == "Stable Diffusion XL":
        if "juggernaut" in nm:
            return e["name"], f"Juggernaut X, an SDXL photo model by {a}.", "A well-known SDXL model for photographic pictures: natural skin, lighting and depth of field." + UNTESTED, ["realism"]
        if "noobai" in nm:
            return "NoobAI XL (vpred 1.0)", f"NoobAI XL, an illustration model by {a}.", "Clean illustration and anime-style pictures with strong line work and colour." + UNTESTED, ["illustration"]
        if "illustrious" in nm:
            return "WAI Illustrious v140", f"WAI Illustrious, an illustration model by {a}.", "Polished illustration and anime-style pictures built on Illustrious." + UNTESTED, ["illustration"]
        if "real" in nm or "photo" in nm:
            return f"Photoreal SDXL · {a}", f"Photographic SDXL model by {a}.", "Aimed at realistic photographs: skin texture, lighting and a camera-like look, in SDXL's small size." + UNTESTED, ["realism"]
        return f"SDXL model · {a}", f"SDXL model by {a}.", "An SDXL checkpoint, small and quick to run." + UNTESTED, []
    return None


def apply(e):
    r = neutral(e) if e.get("mature") else None
    if not r:
        return e
    e["name"], e["tagline"], e["strengths"] = r[0], r[1], r[2]
    e["tags"] = sorted((set(e["tags"]) - ({"illustration"} if "realism" in r[3] else set())) | set(r[3]))
    return e


if __name__ == "__main__":
    p = Path(sys.argv[1] if len(sys.argv) > 1 else "src-tauri/catalog/images.json")
    d = json.loads(p.read_text())
    ms = d if isinstance(d, list) else d["models"]
    for m in ms:
        apply(m)
    p.write_text(json.dumps(d, indent=1 if p.read_text().startswith("{\n ") or p.read_text().startswith("[\n ") else None, ensure_ascii=False))
