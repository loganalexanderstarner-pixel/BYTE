# Changelog

## Unreleased (1.0.0-test.13) — Big models that load, a smoother cloud

- **Big mixture-of-experts models load reliably** (Qwen3.6 35B-A3B, gpt-oss 20B on 16 GB Macs): BYTE now
  leaves enough memory for macOS and the GPU's working buffers when part of a model runs on the CPU, and no
  longer offers versions that can't really fit (it said Qwen3.6 35B-A3B IQ3_XXS would run on 16 GB; it can't).
- **If a model fails to load, BYTE tries safer settings by itself** (smaller context, more of the model on the
  CPU) before showing an error, and remembers what worked.
- **Cloud answers stream smoothly**: the connection to your cloud is kept open between messages, and when the
  cloud closes the answer stream mid-answer BYTE reconnects instantly (it used to wait longer each time and
  give up after five).
- **713 models to choose from** (was 334), and **every model has a Details dropdown**: what it is (from its
  model card), who made it, how strong it is at conversation, writing, coding, reasoning, math, other languages
  and speed, and ideas for using it.
- **Community models**: popular fine-tunes people make (Dolphin, story and role-play models like Cydonia and
  Rocinante, uncensored versions), each with its creator and a plain note on what's different. They sit under
  the Community, Stories and Uncensored filters and are never picked automatically.
- **"Quit these apps" help**: when a model can't load, BYTE lists the apps using the most memory with a Quit
  button for each, and "Try loading again".
- **Cloud documents load faster** (pages aren't downloaded twice, and the cloud connection is reused).
- **Smoother long answers everywhere**: formatting is redrawn about 12 times a second while streaming instead
  of every frame, and the sidebar no longer redraws with every word.

## 1.0.0-test.12 — Web search that works

- **Web search through your BYTE cloud**: with a cloud key saved, every search (including the ones this Mac's
  model makes) goes to the cloud's search first, which asks Google, Bing, DuckDuckGo and Brave at once. Without
  a key, or if the cloud can't help, BYTE searches by itself as before. Private chats never use the cloud.
- **BYTE searches before answering any question about the world**, not only news-like ones, and reads the best
  pages itself after every search instead of hoping the model will.
- **No more raw tool code in answers**, no endless re-searching, and no answers made up from memory when the
  search came back empty.
- **Junk results are thrown away** (a search engine that serves unrelated pages is skipped), login walls and
  empty app pages are skipped, and **Wikipedia** is searched alongside.
- **Weather** comes from a real forecast (Open-Meteo): current conditions and 7 days, °F in the US.
- Follow-up questions ("how much does it cost?") search with the earlier topic.

## 1.0.0-test.11 — Cloud and Both workspaces, byte-ai colors, new logo

- **This Mac · Cloud · Both** at the top of the sidebar (once BYTE Cloud is connected) replaces the Cloud /
  This Mac switch in the chat box. Each chat stays in the workspace it was started in; private chats are
  always on this Mac.
- **Cloud workspace**: lists the conversations on your BYTE cloud (refreshed when you come back to the app),
  opens them, starts new ones and deletes them on the cloud. The chat box shows your plan's modes and how much
  of today's allowance is left.
- **Both workspace**: every question goes to this Mac and your cloud at once. The Mac's answer streams right
  away, the cloud's appears beside it; the cloud's answer continues the chat when it finishes, or pick the
  other with **Keep this one**.
- **byte-ai's colors**: every theme is now five colors (background, panel, border, text, accent) with the rest
  worked out from them. **Midnight** is the new default (existing installs keep their theme), byte-ai's
  Steelers, Ocean, Terminal, Light and Paper are in, and there are **20 themes** in all (new: Aurora,
  Graphite, Fjord, Lavender, Rose, Ember, Mocha, Sand, Mint). Every theme passes a contrast check.
- **New logo**: BYTE's eight-bit mark (one byte, two bits lit) in the app, drawn in the theme's accent, and as
  the app icon.
- **First launch** offers "Use BYTE Cloud instead (invite only)" next to the model list, suggested when no
  model fits the Mac well.
- A spent daily allowance (429) now says so plainly instead of looking like an error. If a cloud answer's
  stream drops and won't come back, BYTE re-reads the conversation and shows the saved answer.

## 1.0.0-test.10 — BYTE Cloud: chat, photos and documents

- **BYTE Cloud**: connect your own BYTE cluster in Settings → Cloud with an API key from
  byteai.bytebylogan.xyz. BYTE checks the key with the cloud first, then keeps it in the macOS Keychain (never
  in a file). Your tier, modes and limits show there.
- **Chat on the cloud**: a Cloud / This Mac switch in the chat box. On the cloud, the mode buttons are the ones
  your account has (Fast, Auto, Extended, Extended+ — whatever your plan includes), answers stream in live, what
  the cloud is doing shows as it works ("searching: …"), and sources appear while the answer is written. Works
  even on Macs with no model downloaded.
- **Cloud answers** carry a small cloud tag and get extra actions: go deeper, explain the reasoning, thumbs
  up/down, and "Answer now" while it's thinking. Edits and regenerate fork the conversation on the cloud too.
- **Import cloud chats** into the sidebar and search.
- **Photos and files** (cloud chats): attach with the paperclip or drop them on the window; **Library** reuses
  photos you've already uploaded without sending them again.
- **Documents on the cloud** (the page icon at the top): PDF reports, slides, Word documents, flyers, worksheets
  and projects. BYTE plans an outline first; you rename, reorder, add or remove sections and pick a design
  (built-in or your own templates) before anything is written — or throw it away at no cost. Progress shows while
  it's made; finished documents preview page by page, download to Documents/BYTE, can be revised with an
  instruction, or turned into another format. A source document (PDF, Word, slides, text) can be used as
  material.
- **Your cloud account on the Mac** (Settings → Cloud): memories, knowledge (text or uploaded files), saved
  prompts, recipes, "about you" and the default mode, search across cloud chats (opens them here), and export
  everything as a .zip.
- **Saved prompts as / commands**: type / in the chat box to pick one.
- **If the cloud can't be reached**, BYTE answers on this Mac instead and says so. A slow start is normal (the
  cluster may be busy) and is never retried into the queue. Private chats never leave this Mac.

## 1.0.0-test.9 — Fastest possible on every Mac, without losing accuracy

- **Models' own speed-up heads**: Gemma 4 (E2B, E4B, 12B, 26B-A4B, 31B), Qwen3.8 27B and Flash-Next,
  gpt-oss 20B/120B and DeepSeek V4 Flash ship a small "multi-token prediction" (or EAGLE-3/DSpark) file
  trained with the model. Speed boost now uses it instead of a separate model: more guesses are right and
  it's a smaller download. In a CPU test Gemma 4 E2B went from ~14.5 to ~22 tokens/sec on an edit.
- **Repeated-text guessing** for every model (no download): when an answer repeats text from the chat —
  code you pasted, a paragraph being fixed — BYTE guesses it ahead. Edits were ~20% faster in tests. Tuning
  keeps it only if it helps on your Mac. Answers never change: your model still checks every word.
- **Recommendations use real measured speed** from tuning on your Mac, count Speed boost, and prefer the
  faster model when quality is equal (big Macs now get gpt-oss 120B at ~60–150 tokens/sec instead of a
  27B at ~10–20). Unless you choose *Faster*, BYTE never picks a model more than a few points less capable
  just to be quicker. Model cards show "measured on this Mac".
- **More models fit**: mixture-of-experts models slightly bigger than the GPU's memory share now run with
  some expert layers on the CPU (a little slower). On 16 GB that adds gpt-oss 20B and Qwen3.6 35B-A3B.
  Dense models a bit too big run in "stretch mode" (noticeably slower; never suggested automatically).
- **Bigger GPU memory share** (Settings → Engine, needs your Mac password, resets on restart): lets the GPU
  use all but 4 GB, so bigger models run fully on it.
- **Thinking is accuracy-first**: in Auto mode BYTE now thinks unless the message is clearly simple
  (hi/thanks, rewrite or translate this, a plain sum the calculator answers), with a short budget for short
  questions and the full budget for reasoning questions.
- If a speed-up helper ever stops the engine from starting, BYTE starts again without it.

## 1.0.0-test.8 — Every model tuned for its best quality and speed

- **Each model family now uses its publisher's recommended settings.** Until now every model used Qwen's
  sampling, which hurt other families. Gemma uses temperature 1.0 with top-k 64, Llama 0.6 / top-p 0.9,
  Mistral Small 0.15, DeepSeek-R1 distills 0.6 / 0.95, LFM min-p 0.15, gpt-oss 1.0, and so on.
- **Thinking works the right way for each model**: switched on/off per question for Qwen-style models,
  always on for reasoning models (R1, QwQ, Phi-4 reasoning…), never forced on models that can't think,
  and gpt-oss gets reasoning effort low / medium / high from the mode.
- **Automatic tuning for this Mac**: the first time a model loads, BYTE spends 1–2 minutes measuring a few
  engine settings on your Mac and keeps the fastest for that model: Speed boost on/off (its helper is
  downloaded automatically), full-precision vs compact conversation memory, and a bigger batch for
  reading long prompts. A banner shows progress; chat waits until it's done. Results and *Tune again* are
  in Settings → Engine; turn automatic tuning off there. Tuning is redone on a different Mac.
- **Thorough tune** (about 5 minutes) also tries the Speed boost look-ahead (8/16/24 words) and confidence,
  flash attention on/off and reading batches of 256–2048. **Tune all** runs it on every downloaded model.
- Fixed: turning flash attention off with Speed boost made the engine exit at startup (the helper's
  memory must be full precision too).

## 1.0.0-test.7 — Speed boost

- **Speed boost (speculative decoding)**: a tiny model from the same family (e.g. Qwen3.5 0.8B for
  Qwen3.5 9B) writes a few words ahead and your model checks them all at once. Your model still decides
  every word, so answers are the same, just faster when the guesses are right. In tests the helper's
  guesses were kept 70–97% of the time. Settings → Engine → Speed; the helper is a one-time download
  (under 1 GB). Only used when it fits in memory next to your model.
- **Test speed on this Mac**: measures real tokens/sec with and without Speed boost and keeps the faster.
  Boosted answers show ⚡ next to their speed.
- **"BYTE's pick favours: Faster / Balanced / Smarter"** in Settings → Models. On a 16 GB M4, *Faster*
  picks Qwen3.5 4B (~27 tokens/sec estimated) instead of Qwen3.5 9B (~13).

## 1.0.0-test.6 — Phase 3 (part 2): edit & versions, projects, profiles

- **Edit any message you sent** (pencil icon): BYTE answers the new version, and the old one is kept.
  **Regenerate** also keeps the earlier answer. Switch between versions with ◀ 1 / 2 ▶.
- **Automatic titles, summaries and tags**: after the first answer BYTE names the chat (e.g. "Quarterly
  taxes") and writes a one-line summary with tags. Hover a chat to see them; search finds them too.
  A title you type yourself is never replaced.
- **Projects**: group chats that share instructions ("Budget is $20k, we like light wood"). Every chat in
  the project follows them. Create one with the folder-plus icon next to *Projects*; move chats in from
  the ⋯ menu.
- **Profiles** (Settings → About): separate chats, memories and settings for different people or for
  work and personal. Downloaded models are shared. Switching restarts BYTE.
- **Interrupted answers**: if BYTE was closed mid-answer, the chat says so and offers *Try again*.

## 1.0.0-test.5 — Phase 3 (part 1): saved chats and memory

- **Chats are saved in an encrypted database** on your Mac (SQLCipher). Chats from earlier test builds are
  moved in automatically the first time.
- **Search every chat**: the sidebar search looks through every message, not just titles, and shows the
  matching sentence.
- **Pin chats, put them in folders, rename them** from the ⋯ menu on each chat.
- **Private chats** (eye icon next to New chat): never saved, and BYTE doesn't use or add memories.
- **Memory**: Settings → Memory & chats has an *About me* box and a list of things BYTE remembers about
  you. BYTE uses them in every chat. When you mention something lasting about yourself, BYTE asks
  *Remember this?* under its answer; nothing is saved unless you click Save. Edit or delete any memory,
  or turn memory off.
- **Export all chats** to a folder (a Markdown file per chat plus one JSON file), or erase everything.
- Models loaded alongside the main one come back automatically after BYTE restarts.
- The calculator runs automatically for sums in your question, so small models can't get the math wrong.

## 1.0.0-test.4 — Hundreds of models, run several at once

- **332 chat models, 1,800 versions, 77 mixture-of-experts.** A 16 GB Mac can run 206 of them, an 8 GB Mac 156.
  Added Llama 3.x/4, Qwen2.5 (and Coder/Math), Gemma 2/3/3n, Mistral Small/Nemo/Large, Ministral, Devstral,
  Phi 3.5/4, DeepSeek R1 distills and V3, GLM 4.x, Granite 3/4, EXAONE, Falcon 3/H1, OLMo, SmolLM, Hunyuan,
  ERNIE, Yi, Aya, Hermes, Nemotron and more. Only official models from their publishers (or faithful
  re-uploads of them); community fine-tunes, role-play and uncensored models are filtered out.
- **Run several models at once.** Downloaded models that fit in the memory left show *Load alongside*.
  Up to three can run next to the main one; BYTE checks the combined memory first.
  - Pick which loaded model answers in the chat box, or **Compare all** to get answers side by side.
  - Settings → Models shows what's in memory now and how much it uses, with *Unload*.
- **Exact math even with small models:** when a question contains a sum ("1234 * 5678", "15% of 80"),
  BYTE runs the calculator itself before the model answers. The Mac engine test caught the tiny test
  model answering 1234 × 5678 = 7,112,932 from memory; with the fix it answers 7,006,652 every time.
- The Check for new models button now says plainly when the online list can't be reached (for example
  when the repository is private); the list built into the app keeps working.
- CI: unit tests run on Linux; the real-engine test runs on macOS only when engine code changes, to save
  macOS minutes on a private repository.

## 1.0.0-test.3 — Big model catalog + chip-aware speed

- **184 chat models, 1,003 downloadable versions**, from 0.4 GB phone-size models to 400+ GB giants, including
  **52 mixture-of-experts (MoE)** models (e.g. Qwen3.6 35B-A3B: 35B knowledge, 3B speed) and low-bit versions
  of big models (Qwen3.8 27B from Q2 to Q8) so bigger models fit smaller Macs. Built automatically from trusted
  publishers on Hugging Face (Qwen, Google, Meta, Mistral, Microsoft, IBM, NVIDIA, DeepSeek, OpenAI, LiquidAI,
  unsloth, bartowski, …). Uncensored, role-play, merged and vision-only models are left out.
- **Speed for every version on your Mac**: estimated tokens/sec and typical answer time (with and without
  thinking), from your chip's memory bandwidth and GPU. BYTE detects the exact chip (M1–M5, Pro/Max/Ultra and
  GPU cores), so an M4 shows faster numbers than an M2. Very slow versions are no longer recommended.
- Each model says what it's **good for**, its size (and active size for MoE), release date and license.
- Catalog browser: **search**, sort by *best for this Mac / newest / smallest / fastest*, **MoE** and
  **Downloaded** filters, "show more" paging, a storage summary (how much disk your models use), and delete for
  any downloaded version. Settings shows the chip, GPU cores and Neural Engine.
- The catalog stays small (~0.4 MB inside the app); model files download from Hugging Face only when chosen.

## 1.0.0-test.2 — Phase 2: web search with sources + model catalog

- **Hardware-aware model catalog**: 12 chat models from 0.5 GB to 63 GB (Qwen3.5 0.8B/2B/4B/9B, Qwen3.8 27B,
  Qwen3.6 35B-A3B, Gemma 4 E4B/12B, gpt-oss 20B/120B, LFM2.5, Qwen3 14B) with several sizes each. BYTE checks
  every version against this Mac's memory, marks it *Great fit*, *Fits*, or *Needs N GB*, and picks
  **BYTE's pick** for your Mac (16 GB → Qwen3.5 9B; 32 GB+ → Qwen3.8 27B). Filter by memory size (8–128 GB)
  or strength (reasoning, coding, writing, languages, fast, small).
- The catalog is a 13 KB list built into the app and refreshed from the web ("Check for new models"); model
  files download from Hugging Face only when chosen. Multi-part files for very large models are supported.
- The memory planner understands hybrid models (only attention layers use context memory), so new Qwen
  models get long contexts cheaply.

- **Web search** without accounts: DuckDuckGo, falling back to DuckDuckGo Lite and then Bing. Requests are
  spaced out so search engines don't throttle BYTE.
- **Page reading**: extracts the main article text and keeps the passages most relevant to your question.
  Results are cached for 24 hours.
- **Tool use**: the model can search, read pages and use an exact **calculator** over several rounds. Fast
  allows 1 round, Auto 3, Deep 6, Extended 10.
- **Grounded answers**: for time-sensitive questions ("latest", "this week", prices, years…) BYTE always
  searches first and reads the top results itself, so answers don't come from stale training data.
- **Citations**: numbered, clickable [1] badges in answers and source cards under them ("read" marks pages
  BYTE opened). Citation numbers the model invents are removed.
- **Activity list** shows live what BYTE is doing (searching, reading, calculating).
- **Web toggle** in the composer; **your name** (set in the welcome guide or Settings → About) for greetings.
- **Safety**: page reading and all internet requests refuse local-network and loopback addresses, including
  through redirects and DNS tricks. Every tool call is recorded in a local action log.
- Answers follow a structure: TL;DR for long answers, headings, numbered steps, bullets, tables, callouts.
  BYTE always calls itself BYTE.
- Tests: 56 Rust unit tests, 18 frontend tests, plus end-to-end tests with a real engine (calculator tool,
  live web research).

## 1.0.0-test.1

First test build of the rebuilt BYTE, for Apple Silicon Macs.

- Built-in AI engine (llama.cpp with Metal); Ollama is no longer needed.
- Model catalog with Qwen3 14B (default), 8B and 30B-A3B, plus helper models.
- Resumable, checksum-verified model downloads with progress, speed and time left.
- RAM planner: checks each model against your Mac's memory, blocks models that can't run, and fits the context window automatically.
- Streaming chat with Fast / Auto / Deep / Extended modes and a thinking toggle; thinking shown in a collapsible panel.
- Tokens-per-second and timing under each answer; stop, copy, and regenerate.
- Welcome guide: Mac check, model choice, download, tips.
- New neon BYTE logo and app icon; 11 themes; text size and density settings.
- Chat history saved on this Mac, grouped by date and searchable.
- Engine settings: status, log, restart, context window size.
- macOS-only release pipeline producing a self-contained `.dmg`.
