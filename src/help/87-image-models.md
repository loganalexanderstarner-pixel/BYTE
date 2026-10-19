# Image models and making pictures

<!-- mac -->
Image models, a catalog of free picture-making models, are part of the Windows app for now. They are not in the Mac app yet.
<!-- linux -->
Image models, a catalog of free picture-making models, are not available on Linux yet, so there is nothing to set up here.
<!-- win -->
Image Studio makes pictures on your PC from a few words. Nothing leaves your PC: not your words, and not your photos. Open [Settings → Images](byte-setting:images).

## Get started
1. Turn on **Make pictures**. It is off until you turn it on, and it stays off in kids mode.
2. Download the **picture engine** once. BYTE recommends the right one for your graphics card (NVIDIA cards use CUDA; any other card uses Vulkan). The file is checked against a fixed checksum.
3. Download an image model under **Image models**. A good first one is **Z-Image Turbo**: quick, photo-real, and happy on small graphics cards.
4. Describe the picture under **Make a picture** and press **Make picture**.

## The switches (all off until you turn them on)
- **Realistic people:** lets you ask for pictures that look like real people. Such pictures can be mistaken for photographs. Don't make pictures of real, named people or use them to mislead anyone.
- **Detail pass:** draws a second, larger pass that adds fine detail in skin, eyes and fabric. About twice as slow.
- **Enlarge each picture:** makes it 2 or 4 times bigger with an upscaler. **4xNomosWebPhoto** is the best for photos of people because it keeps real skin texture. Others smooth skin; some suit art or anime better.
- **Camera finish:** adds what a camera adds (slight softness, colour fringing, flatter colour, a darker corner, fine grain) so pictures look less computer-made. Pick the strength.
- **Where the model lives:** Auto keeps it in video memory while it fits and spills the rest into RAM. Video memory only is fastest. RAM only leaves your graphics card free for games.
- **Keep my pictures:** everything, only the ones you save, or nothing.
- **Remove settings from picture files:** saves pictures without the prompt and settings the engine stores inside them. Good before you share a file.

## Reading a model card
- **The pill** says where the selected version would run: **Fits video memory** (fastest), **Overflows into RAM** (works, slower), **Runs from RAM** (slowest) or **Too big**. The sentence under the versions says by how much.
- **Versions (quants).** Each model comes in several sizes. Smaller ones fit smaller cards and lose a little quality. The star marks the one BYTE recommends for your PC.
- **Good at** badges show what each model stands out for. They never hide models; they just help you choose.

## Finding a model
- **What you want to do:** make pictures, upscale, and more kinds as they arrive (edit a photo, remove things, restore faces, remove a background, change the style).
- **Styles:** Realism (then a real-life look: portraits, street, landscapes, animals, product photos, food, buildings, movie-still or film look), Cartoon, Illustration, Poems and posters, Words in pictures, Fast, Small graphics card.
- **Fits my graphics card**, **Commercial use OK** and **Community models** narrow the list. Community models can include adult ones; those are never shown in kids mode.

## If something goes wrong
- **"Turn on picture making first":** switch on **Make pictures** at the top.
- **"Not downloaded yet":** download that version of the model (the text files it needs come with it).
- **It runs out of memory or crashes:** pick a smaller version, or set **Where the model lives** to Auto.
