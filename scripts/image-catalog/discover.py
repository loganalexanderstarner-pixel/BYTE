import json,urllib.request,sys
def get(u):
    return json.load(urllib.request.urlopen(urllib.request.Request(u,headers={"User-Agent":"byte"}),timeout=30))
Q=[("gguf","filter=gguf&pipeline_tag=text-to-image"),
("flux-dev-q","filter=base_model:quantized:black-forest-labs/FLUX.1-dev&pipeline_tag=text-to-image"),
("flux-schnell-q","filter=base_model:quantized:black-forest-labs/FLUX.1-schnell&pipeline_tag=text-to-image"),
("flux-dev-ft","filter=base_model:finetune:black-forest-labs/FLUX.1-dev&pipeline_tag=text-to-image"),
("sdxl-ft","filter=base_model:finetune:stabilityai/stable-diffusion-xl-base-1.0&pipeline_tag=text-to-image"),
("sd15-ft","filter=base_model:finetune:stable-diffusion-v1-5/stable-diffusion-v1-5&pipeline_tag=text-to-image"),
("sd35m-q","filter=base_model:quantized:stabilityai/stable-diffusion-3.5-medium"),
("sd35l-q","filter=base_model:quantized:stabilityai/stable-diffusion-3.5-large"),
("chroma","search=chroma&filter=gguf"),
("zimage","search=z-image&filter=gguf"),
("qwenimg","search=qwen-image&filter=gguf"),
("flux2","search=flux.2&filter=gguf"),
("flux2k","search=klein&filter=gguf"),
]
out={}
for k,q in Q:
    for pg in range(2):
        try: d=get(f"https://huggingface.co/api/models?{q}&sort=downloads&direction=-1&limit=100&full=false&offset={pg*100}")
        except Exception as e: print(k,e); break
        for x in d: out.setdefault(x["id"],{"id":x["id"],"dl":x.get("downloads",0),"likes":x.get("likes",0),"src":k})
        if len(d)<100: break
print(len(out)); json.dump(out,open("pool.json","w"))
for k,v in sorted(out.items(),key=lambda kv:-kv[1]["dl"])[:70]: print(v["dl"],v["likes"],k,v["src"])
