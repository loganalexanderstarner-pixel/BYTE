# Edit photos, posters, the gallery and more

<!-- mac -->
These picture tools (editing photos, poem posters, a gallery and more) are part of the Windows app for now. They are not in the Mac app yet.
<!-- linux -->
These picture tools (editing photos, poem posters, a gallery and more) are not available on Linux yet, so there is nothing to set up here.
<!-- win -->
These are the extras in [Settings → Images](byte-setting:images). Each one has its own switch, off until you turn it on, and none of them work in kids mode. Everything happens on this PC.

## Prompt helper
Turns a short idea ("a lighthouse at dusk") into a fuller description. Press **A photo**, **A painting**, **A poster** or **A cartoon** under the description box. It uses BYTE's chat model when that is running; otherwise it adds standard details. You can edit what it writes.

## Compare models
Pick more than one model under **Also make it with**, and BYTE makes the same picture with each, one after another, using the same seed. What you see differs because of the model, not luck.

## Gallery
Shows what you made or edited, with what you asked for, which model made it and how long it took. Open a picture to **Save** it (keeps it when "Keep my pictures" is set to only saved ones), **Save a copy…** somewhere else, **Edit this**, or **Delete** it.

## Edit photos
Turn on **Edit photos**, then open the **Edit** tab and pick a picture from your gallery or **Choose a photo…** of your own.
- **Change with words** needs an editing model (download one under Image models → Edit a photo). **Qwen-Image Edit 2511** is the strongest; **FLUX.2 klein 4B** is small and quick. Presets: remove a person or thing, remove the background, restore an old photo, colorize, change the lighting, change the style, change the words on a sign, or anything else you describe.
- **Make bigger** uses an upscaler. It fixes the blocky look of a small or zoomed photo. Choose 4 times (most detail) or 2 times (more natural).
- **Crop**, **Motion blur**, **Film grain** and **Camera finish** need no model.
- Drag the **Before and after** slider to compare. **Keep editing this** continues from the result.

Your original is never changed. Each result is a new picture in the gallery.

## Poem and quote posters
Put a poem on a picture. Pick art from your gallery, or **Draw art for this poem**. BYTE finds the quiet part of the picture (dark and even), sets the words there in a lettering style (Typewriter, Elegant, Neon sign, Dripping paint, Hand lettered), and keeps your line breaks. Long poems flow into columns. You can move the words to another side and change the text size. **Save to my gallery** makes a PNG.

## Draw from chat and Hey BYTE
Turn on **Draw from chat and Hey BYTE**, then say or type "draw me a lighthouse" or "make a picture of a cat in a hat". BYTE makes it with the best picture model you have downloaded and tells you where it is. It never does this in private chats or kids mode.

## If something goes wrong
- **"Turn on Edit photos first":** switch it on at the top of Images.
- **"Pick an editing model first":** download one under Image models → Edit a photo.
- **The first picture takes minutes:** the model is being read from disk for the first time. Later ones are much faster.
