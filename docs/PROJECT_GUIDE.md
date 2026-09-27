# BYTE — Project Guide

This is the complete specification and status of BYTE. It is written so any future Claude Code session (or person)
can pick up the work: what BYTE is, every decision the owner made, the architecture, every planned feature and
which phase builds it, and how everything is verified. Quick-start rules for coding live in `/CLAUDE.md`.

## Phase status

| Phase | Scope | Status |
|---|---|---|
| 1 | Foundation: engine sidecar, model catalog/downloader, RAM planner, streaming chat, modes, thinking, onboarding, logo, themes, CI/release | ✅ Done (commit 47d952e) |
| 2 | Agent & web: tool registry, action log, web search/read, forced grounding, citations, calculator | ✅ Done (test.2) |
| 3 | Memory: encrypted DB, chats, memory/About me, projects, profiles, branching, pins, export | ⏳ Next |
| 4 | Files & knowledge base: parsers, OCR, embeddings, folder indexing, reader | Planned |
| 5 | Documents: PDF/PPTX/DOCX/HTML, edit existing files, infographics, math/diagrams | Planned |
| 6 | Research+: Deep/Extended, academic, quote finder, fact-check, compare, web agent, YouTube | Planned |
| 7 | Writing & learning: studio, long-form, style, flashcards, quizzes, tutor, custom assistants | Planned |
| 8 | Speed: speculative decoding, routing, model lab | Planned |
| 9 | Mac control: Apple apps, Shortcuts, files, clipboard, undo & dry-run | Planned |
| 10 | Upkeep & automation: scheduler, briefing, watchers, trackers, connectors, dashboards | Planned |
| 11 | Input & windows: voice, vision, Quick Ask, floating widget, menu-bar popover, palette, notes | Planned |
| 12 | Privacy & polish: offline, Touch ID, permissions dashboard, 20 themes, sharing, v1.0 | Planned |

## Model catalog (added after Phase 2)

- `scripts/catalog-sources.json` (curated models with quality scores and "used for" text, helpers, quality
  overrides) + `scripts/catalog-discovered.json` (from `scripts/discover-models.mjs`: trusted authors, official
  name allowlist, excludes uncensored/abliterated/RP/vision/merges) → `scripts/build-catalog.mjs` (picks
  variants, drops auxiliary/draft files and duplicate single-vs-split copies, size sanity check, reads the
  GGUF header for architecture via HTTP Range, computes MoE active params) → `src-tauri/catalog/models.json`
  (compiled in, ~0.4 MB). Result: 184 chat models, 1,003 versions, 52 MoE.
- `src-tauri/src/chip.rs`: identifies the chip from the CPU brand + GPU core count (ioreg) → bandwidth,
  GPU TFLOPS, Neural Engine TOPS. Speed estimate: tok/s = bandwidth × efficiency (0.8 dense, 0.6 MoE) ÷ bytes
  of active weights; prompt speed from TFLOPS; typical reply = 1,500-token prompt + 450-token answer
  (+700 thinking tokens). Versions under 8 tok/s get a recommendation penalty.
- The Neural Engine is not used for chat (llama.cpp runs on the GPU); it is planned for OCR and voice.
- The app refreshes the catalog from `settings.catalogUrl` (default: raw GitHub URL of this repo's main branch);
  a newer `generated` date wins and is cached in the data folder. If the repo becomes private, point
  `catalogUrl` at a public host (e.g. the owner's cluster).
- Model keys are `"<id>:<quant>"`; older keys like `"qwen3-14b"` resolve to the Q4_K_M file older builds downloaded.
- Recommendation: highest effective quality (curated or automatic score minus a low-bit penalty, minus a speed
  penalty) among versions that fit comfortably, else that fit at all; ties go to the smaller file.

## Verification log (Phase 2)

- Search parsers tested on real saved DuckDuckGo and Bing result pages; article extraction on a real page.
- Live test from the dev container: 5 real results for a query; rust-lang.org read correctly.
- Real engine + calculator: the model called `calculate` and answered with the exact result.
- Real engine + real web: BYTE searched, read releases.rs, and answered "1.98.1" with a citation.
- Findings fixed: the small model skipped searching and invented citations even with `tool_choice: required`
  (not enforced by this llama.cpp for Qwen3), so BYTE now runs the first search and top-page reads itself for
  time-sensitive questions, and strips unmatched citation numbers.
- Note: the answer cache moves to Phase 4 (it needs the embedding model); the page cache is done.

## Verification log (Phase 1)

- 33 Rust unit tests, 14 frontend tests, production build, strict TypeScript.
- End-to-end test against a real `llama-server` (pinned `b11205`) with Qwen3-0.6B: thinking on/off, API-key auth,
  cancellation (5/5 runs).
- Real Tauri app launched headless: sidecar found, model loaded, engine ready in ~6 s; engine stops on SIGTERM;
  a hard-killed app's orphaned engine is reaped on next launch.
- UI screenshots reviewed (onboarding, model fit, chat, settings, themes) with a mocked backend.
- Bugs found and fixed by these checks: stale keep-alive connections, orphaned engine, paused-download label,
  invisible logo stem.

## Context

The repo holds a Tauri 2 + React + Rust shell called BYTE: one chat box that forwards prompts to an external
Ollama server. No memory, no tools, no model management; the release workflow has failed on every tag (Windows
leg) and only ever produced one Mac `.dmg` (v0.1.6).

Goal (agreed with the user over 10 rounds of feature questions): turn BYTE into **one polished, downloadable
macOS app for M-series Macs with 16 GB RAM that needs nothing else installed**, ships its own inference
engine, runs entirely on the Mac's hardware, searches the web so it stays current, produces real
PDF/PPTX/DOCX files, remembers, reads the user's files, controls Mac apps with permission, and updates itself.
Roughly 120 features were selected; they are catalogued below as modules.

Target: macOS 13+ on Apple Silicon (aarch64) only. Windows/Intel out of scope.

### Decisions locked with the user
- **Engine bundled** (llama.cpp `llama-server`, Metal). Not Ollama.
- **Default model Qwen3-14B Q4_K_M** (~9 GB). Qwen3-8B "Fast" and Qwen3-30B-A3B "Power (tight)" offered.
  Any GGUF can be added; RAM is verified before a model is allowed.
- **Modes Fast / Auto / Deep / Extended** + independent **Thinking** toggle (Auto/On/Off).
- **Web search without accounts** (DuckDuckGo HTML). Page reading built in.
- **No Apple Developer account**: ad-hoc-signed app; README/onboarding explain right-click → Open.
- **Branding: BYTE, cyberpunk neon** (original logo + icon set), 8+ themes.
- **Priority when trading off: answer quality & research depth**, then stability, then looks, then count.
- **Release: build everything, then one public v1.0.** CI produces *hidden pre-release test builds* along the
  way (marked pre-release, never "latest") so the user can test on the real Mac; this is the only way to
  verify a macOS app from this Linux container.
- Opt-in-only modules (off by default, always chat-able): Recipes & meal planning, Mindfulness prompts.

### Known limitations to state plainly in the app and README
- A 14B local model does not write or reason at Claude's level; the *document quality* (templates, layout,
  charts, citations) is where BYTE matches it.
- Speed is bound by memory bandwidth: base M4 ≈ 12–15 tok/s on 14B, 22–28 on 8B; M4 Pro ≈ 2×. Speculative
  decoding adds ~1.5×. Input is processed at ~300–500 tok/s (a 20-page PDF ≈ 15–30 s before the answer).
- Default context 16k tokens (≈12k words), 32k optional (+~1 GB RAM).
- Only one large model fits in 16 GB at a time; vision and "auto-switch" work by **swapping** (10–15 s),
  except on 24 GB+ Macs where two models stay resident.
- Deep/Extended take minutes; a fanless Air throttles ~20–30% on long runs.
- **Unsigned app → macOS permissions (Automation, Accessibility, Microphone, Calendar) must be re-granted
  after each update.** A Developer ID later removes this; the build is prepared for it (one secret).

---

## Architecture

```
BYTE.app
├─ React 18 + TypeScript UI (Vite)                ── invoke / events ──►  Rust core (Tauri 2)
│   • chat, modes, thinking view, sources          • engine supervisor (llama-server, whisper, embed, vision)
│   • module panels (notes, board, tasks, KB, …)   • model manager + downloader (HF, resume, sha, RAM check)
│   • document renderers (HTML→PDF, pptxgenjs,     • agent loop, mode table, research pipeline
│     docx), previews, themes                      • tool registry (~60 tools) + permission gate + action log
│   • command palette, quick-ask & tray windows    • SQLite (SQLCipher): chats, memory, notes, KB, tasks, cache
│                                                  • macOS bridge: AppleScript/JXA, shortcuts CLI, EventKit,
│                                                    Vision OCR, LocalAuthentication, NSSharingService (objc2)
│                                                  • scheduler (cron, watchers, page monitors)
├─ sidecars (prebuilt arm64): llama-server, whisper-cli, sherpa-onnx (KWS + diarization)
├─ optional downloads: models, ffmpeg, yt-dlp
└─ data: ~/Library/Application Support/com.loganstarner.byte/{profiles/<id>/byte.db, models, cache, logs}
```

Principles
- **Rust owns all model I/O and every side effect.** UI never talks to `llama-server` or the OS directly.
- **Every capability is a Module**: `{id, prompt fragment, tools[], permissions[], UI panel?, opt-in?}` registered
  in `src-tauri/src/modules/mod.rs` and `src/modules/index.ts`. Modules can be toggled in Settings → Modules;
  disabled modules contribute no tools or prompt text (keeps context small and fast).
- **Every tool declares a permission scope** (`net`, `fs:read:<path>`, `fs:write`, `apple-events:<app>`,
  `accessibility`, `mic`, `calendar`, `location`, `shell`). The Permission Gate checks the user's grants,
  asks on first use, and logs every call to `actions` (the Permission Dashboard reads this).
- **All outbound HTTP goes through `net.rs`** (one client, offline switch, per-host rate limits, 24 h page cache).

### Engine layer (`engine/`)
- `llama-server` sidecar (pinned ggml-org release, `externalBin`). Args for the main model:
  `-m <gguf> -ngl 99 -fa on -c 16384 -ctk q8_0 -ctv q8_0 --jinja --reasoning-format deepseek --cache-prompt
  --slots 1 --host 127.0.0.1 --port <free>` + speculative decoding when enabled: `-md Qwen3-0.6B-Q8.gguf
  --draft-max 16 --draft-min 2`.
- OpenAI-compatible `/v1/chat/completions` SSE with `tools` and `chat_template_kwargs.enable_thinking`.
  Reuse the reqwest + futures-util streaming pattern from the current `src-tauri/src/main.rs`, rewritten for SSE.
- **Preload**: engine starts at app launch; `/health` polled; warm-up prompt; model stays resident.
- Processes: `main` (chat model), `embed` (nomic-embed, `--embedding`, started on demand, idle-stopped),
  `vision` (Qwen2.5-VL-3B + mmproj, swapped in for image turns on 16 GB; resident on 24 GB+), `whisper-cli`
  (on demand), `sherpa-onnx` (wake word listener when enabled; diarization on demand).
- **RAM planner**: `sysinfo` physical RAM − 4 GB OS − resident processes → decides swap vs co-resident,
  and blocks adding a model whose `file_size + kv_cache_estimate(ctx)` exceeds budget (GGUF header parsed
  with the `gguf` crate for params/quant/arch).
- **Smart auto-switch** (16 GB reality): primary model resident; "simple" turns (classifier on the 0.6B draft
  model) run with thinking off and a short budget; if the user picks 8B as primary, Deep/Extended swap to 14B.
  On 24 GB+, 8B and 14B both stay loaded.
- **Instant-answer cache**: query embedding + mode + attachments hash; cosine ≥ 0.97 and not time-sensitive →
  cached answer (marked "from cache", one click to re-ask). Fetched pages cached 24 h.

### Models catalog (`models.rs`)
| id | source | size | role |
|---|---|---|---|
| qwen3-14b (default) | Qwen/Qwen3-14B-GGUF Q4_K_M | ~9.0 GB | Smart |
| qwen3-8b | Qwen/Qwen3-8B-GGUF Q4_K_M | ~5.0 GB | Fast |
| qwen3-30b-a3b | unsloth/Qwen3-30B-A3B-GGUF Q3_K_M | ~13 GB | Power (warn on 16 GB) |
| qwen3-0.6b | Qwen/Qwen3-0.6B-GGUF Q8_0 | ~0.6 GB | draft model + router |
| qwen2.5-vl-3b + mmproj | bartowski/Qwen2.5-VL-3B-Instruct-GGUF Q4_K_M | ~3.3 GB | vision (swap) |
| nomic-embed-text-v1.5 | nomic-ai Q8_0 | ~0.3 GB | embeddings (KB, cache, ranking) |
| whisper-small.en / base.en | ggerganov/whisper.cpp | 0.15–0.5 GB | voice, transcripts |
| sherpa KWS + diarization | k2-fsa | ~0.1 GB | wake word, speaker labels |
| ffmpeg (static), yt-dlp | optional | ~80 MB | media conversion, YouTube fallback |
Downloader: Range-resume `.part`, progress events, sha256, free-disk check, pause/cancel.
User-added models: file picker or HF URL → GGUF header → RAM check → catalog entry with its own settings
(context, thinking support, temperature) → appears in the mode/model picker with a sub-menu.

### Modes and the agent loop (`agent.rs`, `research.rs`)
| Mode | thinking (Auto) | tool rounds | pages read | output | extras |
|---|---|---|---|---|---|
| Fast | off | 2 | 2 | 1k | cache first |
| Auto | router decides | 4 | 5 | 2k | re-run with thinking if a tool was needed or router says "complex" |
| Deep | on | 10 | 15 (parallel ×6) | 6k | plan 3–6 sub-questions → search → fetch → embed-rank → cited report |
| Extended | on | 20 | 30 (parallel ×6) | 12k | Deep + gap review → follow-up searches → rewrite; sectioned report |
System prompt always carries: date/time, active mode rules, citation format `[n]`, user memory, personality
sliders, active module fragments, profile instructions. Progress events drive a step list in the UI; cancel
at any point; tokens/sec and "explain what happened" data recorded per answer.

### Tool registry (Rust, `tools/`) — grouped by module
- **web**: `web_search` (DDG html + lite fallback, UA rotation, 1 req/s), `fetch_page` (readability via
  `dom_smoothie`, 2 MB cap, PDFs via `pdf-extract`), `academic_search` (arXiv, Semantic Scholar, PubMed,
  Crossref/OpenAlex → CSL-JSON), `youtube_transcript` (innertube captions; fallback yt-dlp+whisper),
  `rss_fetch` (`feed-rs`), `watch_page` (diff via `similar`, price via JSON-LD/regex).
- **compute**: `calculate` (`fend-core`: exact math, units, currency with cached rates), `datetime`.
- **memory/kb**: `kb_search`, `notes_search`, `memory_get/set`, `chat_search` (FTS5).
- **files**: `find_files` (`mdfind` + walkdir), `read_file`, `propose_file_ops` (preview → confirm → apply,
  undo log), `convert` (`sips` images, `lopdf` merge/split, `textutil`+WKWebView for docx→pdf, `zip`),
  `ocr_image` (Apple Vision via `objc2-vision`), `extract_tables`.
- **mac**: `applescript(app, script)` from a vetted script library (Notes, Reminders, Calendar via EventKit,
  Mail read/draft, Messages draft→explicit send, Music, Safari current tab/open URL, System Events: dark
  mode, volume, DND via Shortcuts, Wi-Fi via `networksetup`, sleep via `pmset`), `shortcuts_run/list`,
  `shortcuts_create` (plist → `shortcuts sign --mode anyone` → open for one-tap Add), `open_setting`
  (`x-apple.systempreferences:` URLs), `system_info` (sysinfo, `pmset -g batt`, top processes),
  `storage_scan`, `find_duplicates`, `trash` (`trash` crate), `app_uninstall_plan`, `login_items`.
- **shell**: `propose_command` → UI shows command + explanation + Run button; never auto-runs.
- **selection**: `capture_selection` (hotkey → simulated ⌘C with clipboard restore; Accessibility permission),
  `paste_text`.
- **media**: `transcribe(audio, diarize?)`, `record_screen_steps` (later), `video_keyframes` (ffmpeg).
- **location/weather**: CoreLocation (`objc2-core-location`), Open-Meteo, Nominatim/Overpass (no keys).
- **docs**: `render_document(spec, format, theme)` executed in the frontend (see below); `share`, `print`.

### Data (`db.rs`, SQLCipher via `rusqlite` bundled-sqlcipher, key in Keychain)
Per profile `byte.db`: `conversations`, `messages` (content, thinking, sources, attachments, stats), FTS5;
`memories`; `notes` (+FTS, tags, links); `kb_sources`, `kb_chunks` (embedding BLOB, cosine in Rust);
`projects`; `tasks`, `reminders`; `automations`, `runs`; `watchers`, `feeds`, `feed_items`; `flashcards`,
`reviews` (SM-2); `jobs` (tracker); `clipboard`; `cache_answers`, `cache_pages`; `actions` (permission log);
`settings` (JSON). Migrations versioned. Private chats never touch the DB. Auto-delete after N days optional.

### Documents (`src/lib/docs/`)
Model → **DocSpec JSON** (title, meta, sections: heading/paragraphs/bullets/table/chart/callout/image/quote,
sources) via constrained prompt + JSON repair. Renderers:
- **PDF & shareable HTML & print**: HTML/CSS templates (cyberpunk, clean, paper, academic themes; cover, TOC,
  running headers, page numbers, charts via Chart.js → PNG, sources appendix) → PDF through WKWebView
  `createPDF` (objc2-web-kit, hidden webview). Same HTML is the "shareable page" and the print view.
- **PPTX**: `pptxgenjs` — themed masters, title/agenda/section/bullets (auto-split), native charts, speaker
  notes, sources slide.
- **DOCX**: `docx` — styles, headings, lists, tables, hyperlinks, TOC field.
- **Others**: Markdown/JSON export, flashcards → Anki `.apkg` (sqlite+zip), mind map → PNG/PDF.
Preview panel (split view) → Save as… (`tauri-plugin-dialog`/`fs`, default `~/Documents/BYTE`).
Commands: `/report /deck /doc /brief /compare /flashcards /quiz /mindmap`.

### Windows & Mac integration
- Main window (titlebar overlay, sidebar, chat, right panel). **Quick Ask** floating window (⌥Space, frameless,
  always-on-top). **Tray popover** mini chat (`tauri-plugin-positioner`). Command palette ⌘K (chats,
  settings, modes, templates, actions). Global hotkeys via `tauri-plugin-global-shortcut`; all customizable.
- Deep link `byte://` (`tauri-plugin-deep-link`) for the Finder Quick Action "Ask BYTE" (installed by the app as
  a Shortcuts quick action) and for the web clipper.
- Notifications (`tauri-plugin-notification`), autostart (`tauri-plugin-autostart`), share sheet
  (NSSharingServicePicker via objc2-app-kit), Touch ID (LocalAuthentication), clipboard (`arboard` + change
  count polling).
- Safari helper without an extension: AppleScript reads the front tab URL; BYTE fetches/reads it itself.
- Wake word "Hey BYTE": sherpa-onnx keyword spotter on a low-power audio stream (opt-in; mic indicator shown).

### Design system
Original **BYTE neon logo** (SVG mark: a stylized "B" byte-glyph with glow), app icon set via `tauri icon`,
menu-bar template icon, empty-state illustrations. Tokens in `theme.css` (extend the existing vars):
themes **Neon Night (default), Midnight, Cyber Pink, Terminal Green, Light, Paper, Ocean, Sunset, Forest,
High-contrast, Follow-system**; accent picker; font choice (Inter / JetBrains Mono / system / dyslexia-
friendly). Density compact/comfortable, focus mode, split view, font-size slider. Motion via `framer-motion`
(streaming text, thinking bubble, transitions; respects reduced-motion). Icons `lucide-react`.

---

## Module catalog (everything selected, grouped; each is a self-contained module)

**A. Core assistant** — chat + streaming; modes; thinking toggle & viewer; web search/read; citations & source
cards; explain-what-happened panel (searches, pages, think time, tok/s); cancel; instant cache; parallel
research; speculative decoding; router/auto-switch; preload; example prompts & follow-up suggestions;
adjustable personality (sliders + saved presets); custom instructions & long-term memory (editable list).

**B. Documents** — PDF, PPTX, DOCX, shareable HTML, print; themes; preview; copy-as (MD/plain/RTF/HTML);
share sheet; charts on demand; templates library (cover letter, resume, email, business plan, lesson plan,
agenda, blog, speech, poem, lyrics…, user-added); writing studio (split editor with per-paragraph rewrite/
expand/shorten/tone/grammar); long-form writer (outline → chapters, consistent style); style cloning
(3–5 samples → style profile); poetry/lyrics/speeches with meter & rhyme controls.

**C. Memory & knowledge** — chat history + FTS search + auto-title; project workspaces (instructions, files,
KB scope); file drop (PDF/txt/md/docx/csv/images→OCR); personal knowledge base (folders, watcher, embeddings);
built-in Markdown notes (folders, tags, save-answer-as-note; feeds KB); web clipper (hotkey → reader extract
→ note+KB); built-in reader view with highlights; mind maps & outlines; iCloud Drive backup/restore; export
everything (MD/JSON/HTML); "send to Notes" and iPhone handoff (AirDrop/Notes/Reminders).

**D. Research+** — Deep & Extended pipelines; academic paper search (arXiv/Semantic Scholar/PubMed/Crossref/
OpenAlex) + PDF reading + literature review; citation styles APA/MLA/Chicago/Harvard/IEEE + BibTeX export;
fact-check mode (claims → for/against sources → confidence); compare & decide (weighted table, sliders);
review summarizer; price compare & deal finder; trip planner (itinerary, budget, packing → PDF + Calendar);
local lookup (location + weather + places); YouTube summaries (timestamps, Q&A); video-to-slides.

**E. Learning** — flashcards with SM-2 spaced repetition + Anki export; quiz/exam generator with grading;
tutor mode (Socratic, level-adaptive); instant translate (text/doc/page, selection hotkey); job search tracker
(applications, deadlines, company research, prep).

**F. Planning & automation** — tasks & reminders (natural language, notifications, optional Apple Reminders);
calendar awareness (EventKit); daily briefing (calendar + tasks + weather + followed topics/RSS, scheduled);
news & RSS digest; page watchers & alerts (change/price); scheduled tasks (cron); multi-step agent runs
(plan, progress, checkpoints, ask-when-stuck); visual automations builder (when trigger → steps).

**G. Mac control** — Notes/Reminders/Calendar; Mail inbox triage, drafts in your tone, follow-up tracker;
Messages drafts (explicit send); Music, Safari, system toggles (dark mode, volume, DND, Wi-Fi, sleep);
run any Shortcut; create Shortcuts (one-tap Add); Safari helper (current tab); smart reply anywhere (selection
→ 3 replies → paste); find & organize files (preview → confirm → undo); convert & compress; Finder Quick
Action "Ask BYTE"; clipboard history (200 items, searchable); terminal helper (propose → explain → run with OK).

**H. Mac upkeep** — storage analyzer (treemap, duplicates, big/old files, safe-to-delete explanations, Trash
only with confirm); battery & performance coach; Mac Q&A with open-the-setting buttons; app uninstaller
(bundle + leftovers) & startup/login-items manager; self-diagnostics health check with fix-it buttons.

**I. Input & access** — voice input (whisper, push-to-talk / hold-Space); transcripts with speaker labels
(sherpa diarization) for dropped or recorded audio; vision (paste/drop images, OCR fallback); wake word;
Quick Ask window; tray mini chat; command palette; full keyboard control with customizable shortcuts;
Mac notifications; built-in help center (searchable, offline).

**J. Privacy & profiles** — offline switch (tray indicator); private chats + auto-delete + wipe-all; Touch ID
lock + encrypted DB; permission dashboard (every grant, revoke, full action log); multiple profiles; kids mode
(no web/files, simple UI, PIN); model lab (add any GGUF, RAM check, side-by-side compare); advanced tuning
panel (temperature, context, thinking budget, system prompt editor, live tok/s + RAM/GPU meters).

**K. Optional lifestyle modules (off by default)** — recipes & meal planning (plan, scaling, grocery list →
Reminders, "what can I make with…"); mindfulness prompts (only when asked); brainstorm board (sticky-note
canvas with expand/merge).

**L. Distribution** — macOS-only release workflow; hidden pre-release test builds; auto-update (minisign);
Homebrew tap cask; README + in-app first-open help; CHANGELOG; version sync script.

**M. Added in rounds 11–13 (phase in brackets)**
- Answer quality: self-check pass against sources, best-of-3 drafts, confidence labels (verified/likely/unsure),
  clarifying questions before guessing [2, 6].
- Web agent (built-in hidden WKWebView browser): browse & click, fill forms (always stops for approval before submit),
  download & collect into a folder, full-page screenshots/archives [6].
- Optional connectors, off until signed in: Google Drive/Gmail/Calendar, Notion/Obsidian vault, Dropbox/OneDrive,
  Spotify (OAuth via system browser + loopback redirect; tokens in Keychain) [10].
- Memory: auto-learn preferences (asks before saving), "About me" page, forget on command, memory timeline [3].
- Chat: branch & edit messages, pin & star, side-by-side compare (modes/models), folders & tags + smart folders [3].
- Visuals: LaTeX math (KaTeX), diagrams from text (Mermaid), interactive graphs, step-by-step solver verified by
  `calculate` [5, 7].
- Dashboards: home dashboard, local usage stats, live hardware panel (GPU/RAM/temp/tok/s), research library [10, 12].
- Flair: optional sound effects, custom/animated wallpapers, theme editor with import/export [12].
- Skills: custom assistants, saved prompts as /commands, skill packs (import/export), chained workflows [7, 10].
- Safety nets: undo for every Mac action (activity list), dry-run mode, auto-recover interrupted chats,
  low-battery mode (<20% → faster model, shorter thinking) [3, 9, 10].
- Sharing: .byte chat files, local-network access from another device (password + LAN only), presenter mode [12].
- After 1.0: image generation (Core ML Stable Diffusion), bigger-Mac mode for 32 GB+.
  **Not wanted:** paid Apple Developer ID signing/notarization, iPhone app.
- Rounds 14–15: package & order tracker, bills & subscriptions, gift & event planner, car & home maintenance [10];
  price & deal alerts (wishlists + target prices on page watchers) [10]; game guides with spoiler-free hints [6];
  quote finder (exact supporting sentence per claim) [6]; edit existing PPTX/DOCX, infographics & posters [5];
  hands-free conversation, dictate anywhere [11]. **Skipped:** health features, other money features.
- Every feature is a toggleable module using no memory when off; all stay available.
- Round 16–18 decisions: answers formatted with headings, numbered steps, bullets, TL;DR box, callouts;
  widgets = floating desktop widget + menu-bar popover (no WidgetKit); **unique BYTE layout** with a
  **command-deck home**; 60fps animations (smooth streaming text, spring transitions, neon ambient, micro-interactions);
  **20 themes** across neon, calm dark, light, special; chats get auto one-line summaries + tags;
  identity: always "BYTE", normal (non-cyberpunk) tone, asks the user's name, greets by name, signature touches;
  free **self-signed certificate** (GitHub secret) so TCC permissions persist; user links their Mac for
  Mac-specific phases; docs: CLAUDE.md, docs/PROJECT_GUIDE.md, detailed comments, changelog per phase.
- Next: Phase 2 (web search + sources).

---

## Repository layout (target)

```
src-tauri/src/
  lib.rs, main.rs                 builder, plugins, state; main calls byte_lib::run()
  engine/{mod,llama,whisper,sherpa,vision,ram}.rs
  models.rs  net.rs  db/{mod,migrations,*.rs}  settings.rs  profiles.rs
  agent.rs  research.rs  router.rs  cache.rs  prompt.rs
  modules/{mod.rs, <module>.rs …}   each: tools + prompt fragment + permissions
  tools/{mod,web,academic,compute,files,mac,shell,selection,media,location,docs}.rs
  mac/{applescript,eventkit,ocr,auth,share,clipboard,location,pdf}.rs   (#[cfg(target_os="macos")], objc2)
  scheduler.rs  watchers.rs  permissions.rs  actions.rs  commands/*.rs
src/
  app/ (routes, windows: main | quick-ask | tray | onboarding)
  state/ (zustand stores)  lib/tauri.ts  lib/docs/{spec,html,pdf,pptx,docx,charts,themes,anki}.ts
  lib/files/parse.ts (pdfjs-dist, mammoth, papaparse)
  modules/<module>/ (panel + commands)  components/ (Chat, Sidebar, Composer, Sources, Thinking, Research,
  Export, Notes, Board, MindMap, Tasks, KB, Settings/*, Palette, Help, Diagnostics, Permissions, Profiles)
  design/ (tokens, themes, logo.svg, icons)  help/ (markdown docs bundled)
binaries/ (CI-fetched sidecars)  scripts/ (bump.mjs, fetch-sidecars.sh, cask.rb)  .github/workflows/{ci,release}.yml
```
Cleanup first: remove `src/main.rs`, `src-tauri/memory.json`, template SVGs, inline-style `App.tsx`, the unused
`[package.metadata.tauri]` block; replace `lib.rs` `greet` with the real `run()`.

Key crates: tauri (tray-icon), plugins (shell, dialog, fs, global-shortcut, updater, process, os,
notification, autostart, positioner, deep-link, clipboard-manager), rusqlite (bundled-sqlcipher, fts5),
reqwest 0.12 (rustls), tokio, futures-util, serde, scraper, dom_smoothie, pdf-extract, lopdf, docx-rs,
feed-rs, similar, fend-core, gguf, sysinfo, notify, walkdir, trash, arboard, whatlang, sha2, uuid, chrono,
cron, objc2 + objc2-{app-kit,foundation,web-kit,vision,event-kit,local-authentication,core-location},
keyring, thiserror, tracing, tauri-plugin-log.
Key npm: zustand, framer-motion, lucide-react, marked, dompurify, pptxgenjs, docx, chart.js, pdfjs-dist,
mammoth, papaparse, @codemirror/*, reactflow (board), markmap-lib (mind map), cmdk (palette), i18n later.

Config: `tauri.conf.json` → bundle `dmg`+`app`, `externalBin`, `macOS.minimumSystemVersion 13.0`,
`infoPlist` usage strings (AppleEvents, Microphone, Calendars, Reminders, Contacts, Location), CSP
(no remote scripts; `blob:`/`data:` allowed), `createUpdaterArtifacts`, updater endpoint
`…/releases/latest/download/latest.json`, deep-link scheme `byte`. Capabilities scoped per window.

---

## Build order (all on `claude/new-session-tu1a5x`; each phase = commit(s) + hidden pre-release test build)

1. **Foundation**: cleanup; Rust restructure; `net.rs`; engine supervisor + RAM planner; models catalog +
   downloader; SSE chat with thinking view; onboarding (RAM detect, model pick, download, first-open help);
   new UI shell with design tokens, logo v1, Neon Night theme; macOS-only `release.yml` (pre-release) + `ci.yml`.
2. **Agent & web**: tool registry + permission gate + action log; web_search/fetch_page/calculate; mode table;
   router; Auto-thinking; citations UI; explain panel; cancel; instant cache; page cache.
3. **Memory**: SQLCipher DB + migrations; conversations/FTS/auto-title; memories; projects; profiles (data
   dirs); private chats; export everything.
4. **Files & KB**: file drop parsers; OCR (Vision); embed server; KB indexing + watcher; kb_search; reader view.
5. **Documents**: DocSpec; HTML templates + WKWebView PDF; PPTX; DOCX; shareable HTML; print; copy-as;
   share sheet; charts; templates library; `/commands`.
6. **Research+**: Deep/Extended with parallel fetch + embed ranking; academic search + citation styles +
   BibTeX; fact-check; compare & decide; review summarizer; price compare; trip planner; local lookup
   (location/weather/places); YouTube transcript + video-to-slides (ffmpeg optional download).
7. **Writing & learning**: writing studio; long-form writer; style cloning; poetry/speeches; flashcards +
   SM-2 + Anki; quiz generator; tutor mode; instant translate; job tracker.
8. **Speed**: speculative decoding (0.6B draft); router on draft model; auto-switch policy; tok/s meters;
   tuning panel; model lab (add GGUF, RAM check, side-by-side).
9. **Mac control**: AppleScript library + EventKit; Notes/Reminders/Calendar; Mail triage/drafts/follow-ups;
   Messages drafts; Music/Safari/system; Shortcuts run/create; Safari helper; selection capture → smart reply /
   translate; file find/organize/convert; Finder Quick Action + deep link; clipboard history; terminal helper.
10. **Upkeep & automation**: storage analyzer; battery/perf coach; Mac Q&A; uninstaller/login items;
    tasks/reminders + notifications; calendar awareness; scheduler; daily briefing; RSS digest; page watchers;
    scheduled tasks; multi-step agent runs UI; automations builder; self-diagnostics.
11. **Input & windows**: voice input; diarized transcripts; vision swap; Quick Ask; tray mini chat; command
    palette; keyboard map; wake word; notes editor; web clipper; mind maps; brainstorm board; optional
    lifestyle modules; personality sliders; help center; example prompts.
12. **Privacy & polish**: offline switch; Touch ID + encryption; permission dashboard; kids mode; iCloud backup;
    full theme gallery + animations + layouts; accessibility basics; logo/icon final; README/CHANGELOG/Homebrew
    cask; auto-update channel wiring; version bump to 1.0.0; **public release**.

---

## Verification

Container is Linux: no macOS build/run here. Per phase:
- Local: `npm run typecheck`, `npm run build`, `cargo check` (Linux-compilable; mac-only code behind
  `#[cfg(target_os="macos")]` with stub fallbacks), `cargo test` (search/readability parsers on fixtures,
  DocSpec → renderer smoke tests via vitest, mode table, chunker, RAM planner, SM-2, citation formatters,
  GGUF header parsing), `vitest` for TS.
- CI (`ci.yml`, macOS runner): same + `cargo test` + a smoke `tauri build` (no upload).
- Test builds: tag `v1.0.0-test.N` → `release.yml` publishes a **pre-release** `.dmg` + updater JSON on a
  `test` channel. User checklist on the M4 Air: first-open flow; download resume; each mode answers a current-
  events question with sources; thinking on/off; PDF/PPTX/DOCX open in Preview/Keynote/Pages; file drop; KB
  folder; hotkeys/quick ask/tray; voice; Mac control actions prompt for permission once and work; permission
  dashboard shows the log; offline switch blocks search; Touch ID lock; auto-update to the next test tag;
  Activity Monitor: 14B + 16k ctx ≤ ~11 GB, no swap thrash; 30B-A3B shows the RAM warning.
- Public v1.0: tag `v1.0.0` → stable channel; Homebrew tap updated; README screenshots.

## Deferred (not selected; keep for later)
Read-aloud / hands-free voice reply, screenshot-and-ask hotkey, image generation, Excel export, spreadsheet
analysis, live meeting notes / meeting prep / meeting library, live conversation translation, language tutor,
journal, habits, budget, focus timer, sleep helper, workout planner, HomeKit, AirPlay presenting, printer/
scanner, guided first-run tour, multi-language UI, accessibility pass, wake-word-free "Hey BYTE" alternatives,
crash reports, beta channel, multi-Mac sync, version history, unsubscribe assistant, naming/slogans, decision
matrices beyond compare-and-decide, forms filling, table extraction, contracts explainer, resume builder,
interview practice, essay coach, podcast digest, screen-recording notes, games, story mode, daily fun card,
guest mode, household board, explain-code, formula helper, regex cleanup, Intel build, notarization.
