import json,re,sys
sys.path.insert(0,".")
pool=json.load(open("pool.json"))
cur={ # hand-written entries
"unsloth/Qwen-Image-2.1-GGUF":dict(name="Qwen-Image 2.1",family="Qwen-Image 2.1",community=False,quality=96,steps=20,tags=["realism","illustration","poster","text"],tagline="The most capable free image model, with excellent text inside pictures.",strengths="Follows long, detailed prompts and writes words inside the picture correctly, in English and Chinese. Strong at people, scenes, posters and cards. Large: use a lower quant if your card is small."),
"unsloth/Qwen-Image-2.1-Turbo-GGUF":dict(name="Qwen-Image 2.1 Turbo",family="Qwen-Image 2.1",community=False,quality=92,steps=4,tags=["realism","poster","text","fast"],tagline="Qwen-Image 2.1 in about 4 steps.",strengths="Nearly the quality of Qwen-Image 2.1 in a fraction of the time. A good everyday choice if you have the memory."),
"unsloth/Z-Image-Turbo-GGUF":dict(name="Z-Image Turbo",family="Z-Image",community=False,quality=90,steps=8,tags=["realism","fast","lowvram","poster"],tagline="Fast, photo-real, and happy on small graphics cards.",strengths="Very good photos of people and places in 8 steps, and it fits cards with as little as 4 GB. One of the best picks for speed and quality together."),
"unsloth/Z-Image-GGUF":dict(name="Z-Image (full)",family="Z-Image",community=False,quality=91,steps=28,tags=["realism","illustration","poster"],tagline="Z-Image with more control and variety.",strengths="The full model behind Z-Image Turbo: more varied and easier to steer, but needs more steps."),
"unsloth/ERNIE-Image-Turbo-GGUF":dict(name="ERNIE-Image Turbo",family="ERNIE-Image",community=False,quality=86,steps=8,tags=["illustration","poster","fast","lowvram","text"],tagline="Quick, clean pictures with readable text.",strengths="Good at posters, signs and layouts with words. Runs on cards with 4 GB."),
"unsloth/ERNIE-Image-GGUF":dict(name="ERNIE-Image",family="ERNIE-Image",community=False,quality=87,steps=30,tags=["illustration","poster","text"],tagline="The full ERNIE-Image model.",strengths="Slower than the Turbo version with a little more detail and control."),
"city96/FLUX.1-dev-gguf":dict(name="FLUX.1 dev",family="FLUX.1",community=False,quality=90,steps=28,tags=["realism","illustration","poster"],tagline="The reference model for high-quality images.",strengths="Excellent detail, anatomy and prompt following. Licence is non-commercial."),
"city96/FLUX.1-schnell-gguf":dict(name="FLUX.1 schnell",family="FLUX.1",community=False,quality=88,steps=4,tags=["realism","illustration","poster","fast"],tagline="The best free-for-everything image model of its generation: 4 steps, Apache licence.",strengths="Clean detail, follows long prompts, and you may sell what you make."),
"silveroxides/Chroma-GGUF":dict(name="Chroma",family="Chroma",community=True,quality=82,steps=20,tags=["illustration","cartoon","realism"],tagline="A flexible FLUX-based model with a wide range of styles.",strengths="Open, uncensored-leaning model with very varied styles from anime to photo. Needs more steps than FLUX.1 schnell."),
"second-state/stable-diffusion-v1-5-GGUF":dict(name="Stable Diffusion 1.5",family="Stable Diffusion 1.5",community=False,quality=55,steps=20,tags=["lowvram","fast","cartoon","illustration"],tagline="The classic. Runs almost anywhere.",strengths="Small and quick, works on 4 GB cards, with thousands of styles made for it. Weaker at hands, text and long prompts."),
"second-state/stable-diffusion-3.5-large-GGUF":dict(name="Stable Diffusion 3.5 Large",family="Stable Diffusion 3.5",community=False,quality=82,steps=28,tags=["illustration","text","realism"],tagline="Stability AI's large model with good text and variety.",strengths="Strong, varied pictures and decent text. Large; low quants fit smaller cards."),
"city96/stable-diffusion-3.5-medium-gguf":dict(name="Stable Diffusion 3.5 Medium",family="Stable Diffusion 3.5",community=False,quality=74,steps=28,tags=["illustration","text","lowvram"],tagline="Good text in images and a small file size.",strengths="Better than older Stable Diffusion at words inside pictures and following prompts."),
}
specs=[]
for repo,v in sorted(pool.items(),key=lambda kv:-kv[1]["dl"]):
    if repo in cur: continue
    r=repo.lower()
    if any(k in r for k in ("edit","text-encoder","encoder","controlnet","lora","inpaint","mmproj","vae","-i2i","pe-t2i","pe-i2i","kontext","klein","krea","ideogram","moss","flux.2","flux2","video","wan","ltx")): continue
    from build_image_catalog import family_of
    fam=family_of(repo)
    if not fam or v["dl"]<500: continue
    if "gguf" not in r and "quant" not in r: continue
    name=repo.split("/")[-1]
    base={"Qwen-Image 2.1":"Qwen-Image 2.1","Z-Image":"Z-Image","ERNIE-Image":"ERNIE-Image","Chroma":"Chroma","FLUX.1":"FLUX.1","Stable Diffusion 3.5":"Stable Diffusion 3.5"}[fam]
    tg=["fast"] if "turbo" in r or "schnell" in r or "lightning" in r else []
    if fam in("Z-Image","Qwen-Image 2.1"): tg+=["realism"]
    if fam in("Qwen-Image 2.1","ERNIE-Image","Stable Diffusion 3.5"): tg+=["text"]
    specs.append(dict(repo=repo,family=fam,name=name.replace("-GGUF","").replace("-gguf","").replace("_"," "),tags=tg,dl=v["dl"],
      tagline=f"A community version of {base} by {repo.split('/')[0]}.",
      strengths=f"A community-made version based on {base}. Not tested by us: see its description on Hugging Face ({repo}) for what it changes."))
for repo,c in cur.items():
    c=dict(c); c["repo"]=repo; c["dl"]=pool.get(repo,{}).get("dl",0); specs.append(c)
json.dump(specs,open("specs.json","w"),indent=1); print(len(specs))
