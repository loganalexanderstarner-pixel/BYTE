# BYTE versions

Each version is a phase of the plan (see [PROJECT_GUIDE.md](PROJECT_GUIDE.md)):

- **v0.&lt;phase&gt;.0** is a phase's main build: `v0.5.0` = Phase 5.
- **v0.&lt;phase&gt;.&lt;n&gt;** are improvement builds within that phase: `v0.5.1`, `v0.5.2`…
- **v1.0.0** is the finished app (after Phase 12).

Every version stays downloadable on the [Releases page](../../releases), with a description of what it
has. If you want fewer features (a smaller, simpler BYTE), pick an older version: each one only has the
phases up to its number. Newer versions keep everything older ones had, so a later version is never
"less" than an earlier one.

Release descriptions come from `docs/releases/v<version>.md` (the release workflow reads the file for the
tag it builds).

## All versions

| Version | Phase | What it adds | Good if you want |
|---|---|---|---|
| **v0.6.6** | Phase 6 · Web agent | A private browser BYTE drives for you: opens sites, clicks, fills in forms (asks before submitting), downloads, saves pages as PDF | Getting things done on websites |
| v0.6.5 | Phase 6 improvements | YouTube summaries (chapters, key points, timestamp links) and questions about a video | Watching less, learning more |
| v0.6.4 | Phase 6 improvements | Kitchen (recipes with photos and timers, "what can I make", weekly meal plans, recipe box) and Web Off/Auto/Always | Cooking and meal planning |
| v0.6.3 | Phase 6 improvements | Faster research (searches start sooner, no needless pauses, reuse of recent searches) and a research depth setting | Quicker, deeper research |
| v0.6.2 | Phase 6 improvements | Places nearby (OpenStreetMap cards, open-now) and a trip planner (day plans, budget, packing, PDF, calendar) | Travel and local help |
| v0.6.1 | Phase 6 improvements | Fact-check (verdict per claim with the exact quote) and compare & decide (score table with weight sliders) | Checking claims, choosing between options |
| v0.6.0 | Phase 6 · Deep research | Deep/Extended research (many searches, many pages, ranked passages), research papers, confidence line, citation styles | Thorough, cited answers |
| v0.5.0 | Phase 5 · Documents | PDF / PowerPoint / Word documents written on your Mac, 4 designs, charts, outline you edit first | Documents without the cloud |
| v0.4.0 *(was 1.0.0-test.14)* | Phase 4 · Your files | Attach files and photos, models that see images, scanned PDFs, a knowledge base of your folders, reader view, instant answers | BYTE that knows your documents |
| v0.3.8 *(was test.13)* | Phase 3 improvements | Big mixture-of-experts models load reliably, smoother cloud streaming, 713-model catalog with details, "quit these apps" help | |
| v0.3.7 *(was test.12)* | Phase 3 improvements | Much better web search (searches first, reads pages, weather, Wikipedia, your cloud's search) | |
| v0.3.6 *(was test.11)* | Phase 3 improvements | Cloud and Both workspaces, byte-ai colours, 20 themes, new logo | |
| v0.3.5 *(was test.10)* | Phase 3 improvements | BYTE Cloud: chat, photos, documents, account data | Using your BYTE cloud from the app |
| v0.3.4 *(was test.9)* | Phase 3 improvements | Fastest settings per Mac, speed-up heads, big models partly on the CPU | |
| v0.3.3 *(was test.8)* | Phase 3 improvements | Every model tuned for its best quality and speed | |
| v0.3.2 *(was test.7)* | Phase 3 improvements | Speed boost (a helper model guesses ahead) | |
| v0.3.1 *(was test.6)* | Phase 3 · Memory, part 2 | Edit and branch messages, projects, profiles, recovered answers | |
| v0.3.0 *(was test.5)* | Phase 3 · Memory | Saved, searchable, encrypted chats; memory and "About me"; private chats; export | Chat + web search + memory, nothing more |
| v0.2.2 *(was test.4)* | Phase 2 improvements | Hundreds of models, run several at once | |
| v0.2.1 *(was test.3)* | Phase 2 improvements | Big model catalog, chip-aware speed estimates | |
| v0.2.0 *(was test.2)* | Phase 2 · Web | Web search with sources, calculator, modes | A simple private chat that can search |
| v0.1.0 *(was test.1)* | Phase 1 · Foundation | Built-in engine, model download, chat with thinking, themes | The smallest BYTE: just local chat |

Versions before v0.5.0 were published under the old test names (`v1.0.0-test.N`); the "was" column says
which download is which. Their files and notes are unchanged.

## Coming next

| Version | Phase |
|---|---|
| v0.7.0 | Writing & learning |
| v0.8.0 | Speed |
| v0.9.0 | Mac control |
| v0.10.0 | Upkeep & automation |
| v0.11.0 | Input & windows (voice, vision, quick ask) |
| v0.12.0 | Privacy & polish |
| v1.0.0 | The finished BYTE |
