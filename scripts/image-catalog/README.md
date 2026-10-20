# Image catalog data

`src-tauri/catalog/images.json` is generated, not edited by hand.

1. `discover.py` searches Hugging Face for image-model repositories and writes `pool.json`.
2. `make-specs.py` turns the pool (plus the hand-written entries in it) into `specs.json`.
3. `../build-image-catalog.py specs.json` reads each repository's file list and writes the catalog, with the size and
   SHA-256 Hugging Face publishes for every file. Upscalers are listed in `build-image-catalog.py`.

`specs.json` here is the list used for the checked-in catalog; run step 3 alone to refresh sizes and checksums.

`more-checkpoints.py` adds community Stable Diffusion checkpoints (one `.safetensors` file) from a pool of Hugging Face repositories to `specs.json`; edit models (`kind: "edit"`) and FLUX.2 klein are hand-written entries.
