# Changelog

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
