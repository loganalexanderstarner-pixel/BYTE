# BYTE — handoff for the next session

**Read this first, then `CLAUDE.md` (rules), `docs/PROJECT_GUIDE.md` (full spec, architecture, feature
catalog), `docs/DESIGN-AND-PLATFORMS.md` (byte-ai's palette and logo, and the Windows/Linux plan) and
`docs/CLOUD-MODE.md` (the cloud API contract and how the cluster behaves).** This file says where things stand, how to work
on the project, and exactly what to build next. It contains no secrets and must never contain any (the repo
is public, with history).

Last updated: 2026-09-28. Owner decisions since test.10 (recorded by another session in
`docs/DESIGN-AND-PLATFORMS.md` / `docs/CLOUD-MODE.md`): **Windows and Linux are now targets** (macOS first,
ports after 1.0, define a `ModelBackend` boundary now; **no iOS**); **adopt byte-ai's palette and eight-bit logo** (two rows of four squares, two lit — not a bolt)
(keep the layout and many themes); cloud sign-up is **invite-only**; every phase is still built natively.

---

## 1. What BYTE is (one paragraph)

A macOS app (Apple Silicon only, Tauri 2 + Rust + React/TypeScript) that runs AI models **on the Mac** with a
bundled llama.cpp engine (Metal), searches the web without accounts, remembers (encrypted SQLite), and — new —
can also use the owner's **BYTE cloud** (a self-hosted cluster, `https://byteai.bytebylogan.xyz`) as a remote
backend through an API key. Target machine: MacBook Air M4, 16 GB. The assistant is always called **BYTE**,
talks in a normal friendly tone; the *visuals* are neon/cyberpunk. No paid Apple Developer account, ever.

## 2. Where things stand

| Area | State |
|---|---|
| Phases 1–3 (engine, models, chat, web search, agent, calculator, encrypted chats, memory, projects, profiles) | ✅ done |
| Speed work (Phase 8 pulled forward): Speed boost with family drafters **and models' own MTP/EAGLE-3/DSpark heads**, repeated-text guessing (ngram), per-Mac auto-tuning (quick/thorough/tune-all), measured-speed recommendations with a quality floor, CPU offload for MoE (`--n-cpu-moe`) + stretch mode, optional bigger GPU share, accuracy-first thinking router | ✅ done (test.7–test.9) |
| Cloud mode (chat, streaming, actions, attachments + library, documents with outline approval + templates, account data, `/` prompts, fallback to local) | ✅ built (test.10) — **not yet verified against the real cluster** (see §6) |
| Cloud redesign: This Mac · Cloud · Both workspaces, server chat list with new/delete, Both answers side by side with "Keep this one", budgets chip, 429 message, re-read after a dropped stream, invite-only onboarding path | ✅ built (for test.11) |
| Phases 4–7, 9–12 (files & knowledge base, local documents, research+, writing/learning, Mac control, automation, voice/windows, privacy & polish) | ⏳ planned — every phase in detail in §8 |

Test builds are GitHub pre-releases `v1.0.0-test.N` (latest: test.13). The owner installs them on the M4 Air
and reports back with screenshots; this environment can't run macOS.

## 3. Next work, in order

### 3.1 Check test.10
`release.yml` for `v1.0.0-test.10` and `mac-engine.yml` on the branch head. test.10 is the first macOS build of
the `keyring` crate (Keychain). If it fails, fix it before anything else.

### 3.2 Owner's cloud requests — B and C done, A done as far as the app can go

**A. "It should work when anyone downloads it."**
- Local mode already needs nothing but a model download. Make first launch offer two paths:
  **"Run on this Mac"** (current onboarding) or **"Use BYTE Cloud"** (paste a key; link that opens
  byteai.bytebylogan.xyz to create an account/key). Macs too small for a good local model should be steered to
  the cloud path (use `system_info` + `models::recommend` returning None or a weak model).
- Nothing may be tied to the owner: **never ship a key inside the app.** Anything in a public download can be
  pulled out, and a built-in key would let anyone use the owner's account and quota. A key the owner pastes
  lives only in *their* Mac's Keychain; other people's copies never see it.
- So "cloud works for anyone" needs a way for each user to get *their own* credentials. Options, best first
  (all need backend work on the byte-ai side, which the owner controls):
  1. **Sign in from the app**: "Sign in to BYTE Cloud" opens the browser to byteai.bytebylogan.xyz, the user
     logs in or signs up, and the site hands the app a per-user token (OAuth-style loopback redirect or a
     device code). The app stores it in the Keychain like the key today. Best experience.
  2. **Free guest tier**: on first launch the app asks the backend for an anonymous, rate-limited per-install
     token (e.g. `POST /api/auth/device`), so the cloud works with zero setup; signing in upgrades it.
  3. **Bring your own key** (what exists now): each user signs up on the website and pastes their key.
  Decide with the owner which of these the backend will support, then build the app side (the Keychain
  storage, `/api/auth/me` validation, modes-from-account and fallback already exist).
- **Answered:** sign-up is **invite-only** (no open registration), so onboarding must say "BYTE Cloud needs an
  invite" rather than implying anyone can use it. Show the account's budgets; a 429 means an allowance is
  spent, not an error.
- **Done in the app:** onboarding step 2 has "Use BYTE Cloud instead (invite only)" (key paste, suggested when
  no model fits); budgets chip in the chat box; 429 → "today's allowance for this is used up".
  **Still open (backend first):** option 1 or 2 above; the app side is small once the server issues per-user
  tokens (store them exactly like the key).

**B. Cloud as its own tab, not a toggle.** ✅ done — see PROJECT_GUIDE "Workspaces". New cloud chats are
created lazily (the first message posts `POST /api/conversations`), so empty chats never reach the server.
**Backend ask:** `DELETE /api/conversations/{id}` isn't in the contract; the app calls it and, on 404/405, says
deleting isn't supported and hides the chat locally. Add the endpoint on the server (or tell us the real one).
- Replace the composer's Cloud/This Mac pill with a top-level workspace switch in the sidebar header:
  **This Mac · Cloud · Both** (persist as a setting, e.g. `workspace`).
- **Cloud tab:** the chat list comes from the server (`GET /api/conversations`, source of truth), with a
  **New conversation** button (`POST /api/conversations`) and **Delete** per conversation. Delete isn't in
  `docs/CLOUD-MODE.md` — try `DELETE /api/conversations/{id}` and confirm with the owner. Opening one loads
  `GET /api/conversations/{id}`. Mode buttons are the account's `me.modes` (Fast / Auto / Extended /
  Extended+ as the key allows). Reuse the existing streaming (`cloud::cmd::send`, `cloud::follow`), message
  actions and documents; keep a local mirror only for offline reading/search.
- **This Mac tab:** today's local experience, unchanged.

**C. "Both" tab: cloud and local together, for better and faster replies.** ✅ done as described below
(the cloud side uses `noFallback`, so an unreachable cloud shows an error beside the Mac's answer instead of a
second local answer).
- Recommended design: send each question to **both**; show the local answer as soon as it streams (fast), and
  the cloud answer side by side when it arrives (usually better). Reuse the side-by-side compare UI that
  already exists for multiple local models (`store.answer` with a shared `group`, `compare-grid`). The user
  picks which answer continues the conversation ("Keep this one"); default to the cloud answer when it
  finishes, the local one if the cloud is unreachable.
- Optional later: local writes a quick draft while the cloud "refines" it (send the draft as context).
- Private chats never go to the cloud, in any tab.

### 3.2b Also next (owner decisions from 2026-09-27)
- ✅ **byte-ai skin** (done: `styles/tokens.css`, `design/themes.ts`, contrast test `styles/tokens.test.ts`;
  needs `color-mix`, fine since the minimum macOS is 13.3). Was: (`docs/DESIGN-AND-PLATFORMS.md` Part 1): five-token themes with derived `color-mix` values,
  byte-ai's themes added (Midnight becomes the default), the eight-bit logo (✅ done: `src/design/Logo.tsx` + app icon) drawn in the
  live accent, a contrast check for every theme. Keep the layout and the many themes.
- ✅ **`ModelBackend` boundary** (done: `src-tauri/src/backend.rs`, `LocalLlama` + `Cloud`, fallback rules in
  `backend::with_fallback`, tested with fake backends). Was (Part 2): one trait for "answer this turn" with the local engine and the cloud as
  implementations; `chat_send` goes through it. No Windows/Linux ports before 1.0, but new code must not add
  macOS assumptions outside `#[cfg(target_os = "macos")]` modules.

### 3.2c Web search (done 2026-09-28, for test.12)
The owner reported web search "doesn't work well at all". A live report (`agent::tests::e2e_web_quality_report`,
real engine + internet; `BYTE_TEST_WEB=1 BYTE_TEST_LLAMA_SERVER=… BYTE_TEST_MODEL=…`) went from 1/8 usable answers
to 7/8 after: forced search for any question about the world (`router::wants_web`), BYTE reading pages itself
after every search (`agent::read_top`), a search budget and no repeat searches, an "answer now" step plus a filter
for tool-call markup (`agent::ToolTextFilter`), a junk-result filter (`search::on_topic`: Bing serves unrelated
pages to bots), Wikipedia alongside, Open-Meteo weather (`tools/weather.rs`), and the cloud's `/api/search`
(SearXNG) as the primary source when a key is saved (`docs/CLOUD-MODE.md` "Web search").
**Server-side notes from the owner's session:** the searxng-settings ConfigMap holds SearXNG's `secret_key`
in plaintext; it belongs in a Secret (low risk, internal-only).

### 3.2d Speed and reliability list (owner, 2026-09-28: "add more speed fixes to the list")
Done (9f854b3): honest memory plans for CPU-offloaded models (4 GB for macOS, 1 GB GPU headroom), a startup
fallback ladder (`engine::fallback_launches`), a shared cloud HTTP client, instant reconnects when the cloud
closes a stream mid-answer, markdown re-rendered ~12×/s while streaming, sidebar redraws only on list changes.
All 58 catalog architectures are supported by the bundled llama.cpp (b11205).
Next, in order:
1. **Cloud: open the answer stream while the message is being posted** (today BYTE waits for
   `POST …/messages` to return before it starts listening; if the server does work before replying, the first
   words arrive late). Needs `since` = the last known message id; check with the server session that the stream
   delivers rows posted after it opened.
2. **Model lab** (add any GGUF by file or Hugging Face link, with the RAM check) and the **advanced tuning
   panel** (temperature, context, thinking budget, live tok/s and memory).
3. **Low-battery mode**: under 20% battery, a faster model and shorter thinking.
4. **Show why a model can't run** right in the model list (the planner's note), and a "Test load" button.
5. Later: Apple MLX engine (10–20% faster on some models; a big rebuild).

### 3.2e Owner ideas to build (recorded 2026-09-28)
**Vision models** ✅ built with Phase 4 part 1 (see §8 Phase 4): the catalog says which models can see images (they ship an `mmproj` file;
`build-catalog.mjs` finds it, ModelCard shows "Sees images"); the engine passes `--mmproj`. Photo/file buttons
in the chat box appear only when what's loaded can use them (a vision model for photos; any model for text
from files). Cloud keeps its existing attachments.

**Image generation** (right after Phase 4):
- Engine: `stable-diffusion.cpp` (Metal) as a second sidecar, built and pinned like `llama-server`.
- A big image-model catalog (SD 1.5, SDXL, SD 3.5, FLUX, newer GGUF image models), built like the chat catalog
  (discover → build → enrich), with the same fit planning, including next to a loaded chat model.
- **A 4th workspace, "Image"**: the sidebar switch becomes This Mac · Cloud · Both · Image, and Image appears
  when an image model is loaded. Combinations the owner wants: image model alone; image model + local chat
  model; image model + BYTE Cloud; chat model alone. When a helper is chosen (local model or cloud), it turns a
  short request into a detailed prompt first (shown, editable), then the image is made.
- Gallery: saved to `~/Pictures/BYTE`; open, copy, share, delete; re-run with the same seed; size/steps presets.
- **Owner, 2026-09-28: use the Mac's hardware, and add upscaling / refining.**
  - Hardware: image models run on the GPU through Metal (stable-diffusion.cpp's Metal backend, or MLX later);
    Core ML versions (Apple's ml-stable-diffusion) can also use the **Neural Engine**, which frees the GPU and
    saves battery. Benchmark both per Mac, like `tune.rs` does for chat, and keep the faster one. The M4's
    **hardware ray tracing doesn't help**: it speeds up 3D rendering (light rays bouncing through a scene),
    and image models are pure matrix math, which runs on the GPU's compute units and the Neural Engine.
  - **Upscale to 4K**: a small upscaler model (Real-ESRGAN / 4x-UltraSharp class; stable-diffusion.cpp has
    `--upscale-model`) turns a 1024 px image into 4096 px in seconds. Offered as "Upscale ×2 / ×4" on
    every image in the gallery.
  - **"More detail" / "More realistic"**: run the finished image back through the model (image-to-image) at
    low strength (~0.25–0.4), optionally in tiles at the higher resolution ("tiled upscale", keeps memory
    low on 16 GB). Presets: *More detail* (same prompt, upscale then refine), *More realistic* (adds
    photo-style wording and a realism-tuned model/LoRA when installed), *Variation* (higher strength).
    The chat model or BYTE Cloud can rewrite the prompt for the refine pass, as in the first pass.

**Windows and Linux**: separate native apps per OS, and flexible RAM/VRAM use: see
`docs/DESIGN-AND-PLATFORMS.md` "Owner decisions, 2026-09-28".

### 3.3 Then Phase 4 — Files & knowledge base
Full plan in §8 (Phase 4). After that, Phases 5–12 in order (§8). The owner prioritizes **answer quality**,
then stability, then looks, then feature count.

## 4. How to work on this repo

- **Branch:** `claude/new-session-tu1a5x` (all work goes here; `main` gets it through PRs the owner merges).
- **Tooling on a Linux server:** Node 22, Rust stable, and Tauri's Linux libs:
  `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev`.
  `tauri-build` needs a file at `src-tauri/binaries/llama-server-<host-triple>`: run
  `scripts/build-llama-server.sh` (builds the pinned llama.cpp from `scripts/LLAMA_TAG`) or create an empty
  placeholder for unit tests only.
- **Checks before every push:**
  `npm run typecheck && npx vitest run && npm run build`, `cd src-tauri && cargo test --lib`,
  `scripts/check-secrets.sh`. Set `CARGO_INCREMENTAL=0` if disk is tight.
- **Real-engine tests** (ignored by default): see the env vars in `.github/workflows/mac-engine.yml`
  (`BYTE_TEST_LLAMA_SERVER`, `BYTE_TEST_MODEL` = Qwen3-0.6B Q8_0, `BYTE_TEST_MAIN_MODEL`/`BYTE_TEST_DRAFT_MODEL`
  = Qwen3.5 2B + 0.8B Q4_K_M, `BYTE_TEST_HEAD_MAIN`/`BYTE_TEST_HEAD` = Gemma 4 E2B Q3_K_M + its MTP head).
  All are public Hugging Face downloads (URLs in the workflow). Run with `cargo test e2e -- --ignored
  --test-threads=1`. Don't run two engines at once on a small box.
- **UI screenshots:** `tools/ui-shots/` (see its README): build, serve with `npx vite preview`, run
  `node shots.mjs`; it mocks every Tauri command and walks the main screens. Add mocks for new commands, and
  look at the pictures before claiming UI works.
- **The repo is public (since 2026-09-28), so Actions is free.** `ci.yml` runs once per push (docs-only pushes
  skip it), `mac-engine.yml` on engine-related changes, `release.yml` by hand for test builds. Before every push
  run `scripts/check-all.sh`; log every commit in `docs/WORKLOG.md`. `scripts/build-mac.sh --install` builds on
  the owner's Mac without GitHub at all.
  Batch commits and push less often; run the real-engine tests locally where you can
  (`BYTE_TEST_LLAMA_SERVER`/`BYTE_TEST_MODEL`, see chat.rs).
- **Test build:** `node scripts/bump.mjs 1.0.0-test.N` → commit → push → run the `release.yml` workflow with
  input `tag = v1.0.0-test.N` (workflow_dispatch). Add a CHANGELOG entry per build, in plain language.
- **Style:** match the surrounding code; no repo-wide reformatting (there is no prettier config — don't run
  prettier on whole files). Tests next to the code (Rust unit tests; `*.test.ts` for TS).

## 5. Map of the code (where things live)

- Engine & speed: `src-tauri/src/engine.rs` (launch args, `LaunchOpts`, `Draft`, retry without helper),
  `tune.rs` (per-Mac tuning), `speed.rs` (measurement), `models.rs` (catalog, helpers, scoring, offload plan),
  `system.rs` (RAM/GPU planner, `plan_offload`, GPU share), `chip.rs` (speed estimates).
- Catalog: `src-tauri/catalog/models.json`: `discover-models.mjs` (+ `--community`) → `build-catalog.mjs` →
  `enrich-catalog.mjs` (details dropdown data). See PROJECT_GUIDE "Model catalog".
- Chat & agent: `chat.rs` (SSE client, `ChatEvent`), `agent.rs`, `router.rs` (thinking effort), `prompt.rs`,
  `tools/` (web search, page reading, calculator).
- Data: `db.rs` (SQLCipher, schema v3 with `cloud_id`), `settings.rs`, `profiles.rs`, `export.rs`.
- Cloud: `src-tauri/src/cloud/` (`mod.rs` client + stream follower, `sse.rs`, `keychain.rs`, `cmd.rs`
  commands incl. checked `/api/` proxy, `tests.rs` with wiremock); UI in `src/components/settings/CloudTab.tsx`,
  `CloudAccount.tsx`, `src/components/documents/DocumentsPanel.tsx`, `src/components/chat/Attachments.tsx`,
  helpers `src/lib/cloudDocs.ts`, store logic `src/state/store.ts` (`cloudTurn`, `streamReply`, `cloudAct`,
  attachments, saved prompts).

## 6. Known unknowns and gotchas

- **Cloud reply shapes are guessed where the contract is silent** (fields of `/api/auth/me`, job status names,
  outline shape, preview page numbering (1- vs 0-based), attachment/document field names, feedback body,
  where the mode goes in a message post). Parsing is tolerant (`cloud::parse_me`, `posted_ids`,
  `src/lib/cloudDocs.ts`). When the owner reports a mismatch, fix the parser and add a test with the real shape.
  If a session has access to the byte-ai backend source, read its route/schema definitions to confirm.
- **Never ask for, accept, use, or store a real API key.** Build against mocks; `GET /api/auth/me` returning
  401 without a key is the only live check needed. If someone pastes a key into a chat, don't use it and tell
  them to revoke it. Tests use short fake keys (`byte_test_…`).
- The local engine client must not reuse connections (`chat::local_client`).
- llama.cpp flags: a q8 draft/MTP cache needs flash attention on; `ngram-mod` defaults (24-token match) suit
  code, BYTE uses a 4-token match for chat.
- Memory: 16 GB Macs have ~10.7 GB of GPU share; `system::plan_offload` handles models just over it.
- `pkill -f` can kill your own shell in some sandboxes; kill by PID.

## 7. Owner preferences (short)

Plain-language updates; ask before large or irreversible changes; say clearly what was verified and what
wasn't; screenshots for UI work; one test build per meaningful chunk of work. The owner has the Mac for
testing and a Kubernetes cluster running the BYTE backend.

## 8. All phases in detail

Rules for every phase: each feature is a **module** the user can switch off (off = no prompt text, no tools,
no memory use); Rust owns every side effect; tests next to the code; screenshots for UI; CHANGELOG entry and a
test build `v1.0.0-test.N` per phase (or per meaningful chunk); update this file's status table (§2) and the
phase status in `docs/PROJECT_GUIDE.md`. Anything touching macOS APIs goes behind
`#[cfg(target_os = "macos")]` with a Linux stub so CI keeps compiling, and is verified by the owner on the Mac.
Mac permissions (Automation, Accessibility, Microphone, Calendar…) are asked the first time a feature needs
them, with an explanation; every tool call that acts on the Mac is logged in the action log.

### Phase 1 — Foundation ✅ done
Built: Tauri 2 shell (Rust core + React UI), bundled `llama-server` sidecar (pinned llama.cpp in
`scripts/LLAMA_TAG`, Metal) with supervisor (free port, per-launch API key, health checks, warm-up, crash
restart, PID file + stale-engine reaping, stop on quit/signals); model catalog (`src-tauri/catalog/models.json`,
~330 models with sizes/variants/sharded files, refreshed from GitHub) and a resumable SHA-256-verified
downloader; RAM/GPU planner (fit, context size); streaming chat (SSE) with a thinking view, modes Fast / Auto /
Deep / Extended and a Thinking toggle; onboarding (Mac check, model pick, download); neon UI with logo and
themes; CI and a macOS release workflow producing pre-release `.dmg` test builds.
Key files: `engine.rs`, `models.rs`, `system.rs`, `chip.rs`, `chat.rs`, `settings.rs`, `src/components/onboarding/`.

### Phase 2 — Agent & web ✅ done
Built: tool registry + agent loop (`agent.rs`, `tools/`): `web_search` (DuckDuckGo, no account),
`fetch_page` (readability extraction), `calculate` (fend-core, exact math/units); numbered citations with
source cards; activity list ("searched…, read…"); forced grounding — BYTE searches first when a question needs
current info (`router::needs_fresh_info`) and runs the calculator itself for sums (`router::math_expression`),
because small models skip tools; action log (`actions.jsonl`); page cache; cancel.
Not done from the original Phase 2 list (moved later): instant-answer cache (needs embeddings → Phase 4),
answer self-check / best-of-3 / confidence labels (→ Phase 6).

### Phase 3 — Memory ✅ done
Built: encrypted SQLite (SQLCipher, key file `db.key` 0600), chats with FTS5 search, pins, folders, private
chats (never saved, no memory), long-term memories + "About me" + "Remember this?" suggestions, export
(Markdown/JSON), edit & versions (◀ 1/2 ▶), automatic titles + one-line summaries + tags, projects (shared
instructions), profiles (separate data folders), interrupted-answer recovery, load several models at once and
side-by-side compare. Key files: `db.rs` (schema v3), `summarize.rs`, `profiles.rs`, `export.rs`,
`src/lib/branches.ts`, `src/state/store.ts`.
Left for later: memory timeline, "forget on command" in chat, smart folders.

### Phase 4 — Files & knowledge base ⏳ in progress (part 1 done: attachments + vision)
Goal: BYTE reads the user's files — attached to a chat or from folders they choose — and cites them.

**Part 1 done (2026-09-28):** item 1 below plus vision models. As built: one module `src-tauri/src/files.rs`
(not a folder); `file_ingest` command; `ChatMessage.files` carried in the wire messages, turned into
`<file name=…>` blocks by `chat::with_files` (latest message ~45% of the context, older ones ~8%, best
passages via `relevant_passages`); photos go to the model as OpenAI image parts only when the engine was
started with `--mmproj` (`Endpoint.vision`, `EngineStatus::Ready.vision`), at most 4 from the latest message.
Catalog `vision.file` (mmproj; `build-catalog.mjs pickVision`, `--vision` re-checks in place; 128 models),
download key `"<id>:vision"` into `models/vision/<id>/` (repos reuse the name `mmproj-F16.gguf`),
`tune::launch_opts` adds it when downloaded, the engine's last fallback drops it. HEIC/WebP photos are
converted and big photos shrunk with macOS `sips`. UI: paperclip + drop in local chats (photos only when the
loaded model can see), `LocalFileChips`, "Sees images" tag and image-reader row on model cards. Not yet: paste,
local files in Both chats (cloud side doesn't get them).

**Item 2 (OCR) done:** `src-tauri/src/ocr.rs` (Apple Vision `VNRecognizeTextRequest`, accurate + language
correction; PDFKit renders scanned pages at ~200 dpi, max 60 pages; non-macOS returns "needs macOS").
`files::ingest` uses it when a PDF has under ~20 letters per page (`has_text_layer`) and for words in photos;
`Ingested.ocr`. Mac-only code: type-checked here with a scratch crate (`cargo check --target
aarch64-apple-darwin`), compiled and tested for real by `mac-engine.yml` (`ocr::tests::e2e_reads_text_from_a_rendered_page`
is not ignored on macOS).

**Items 3–7 done (embeddings, knowledge base, `search_my_files`, reader, KB settings):**
- `embed.rs`: `Embedder` (own `Engine::helper`, PID file `embed.pid`, `LaunchOpts.embedding` → `--embedding
  --pooling mean`, FA auto, f16 cache, no chat flags), started by the first `embed()`, stopped after 5 idle
  minutes; nomic prefixes `search_document:`/`search_query:`; vectors normalized. Real-engine test
  `embed::tests::e2e_embeddings_find_the_passage_that_answers` (local Linux: 0.73 vs 0.45/0.51; also in
  `mac-engine.yml`). The Linux dev engine is built with `GGML_NATIVE=ON`: after a container moves to another
  CPU it dies with "Illegal instruction"; delete `.cache/llama.cpp/src-*/build` and rebuild.
- `kb.rs` + DB schema **v4** (`kb_sources`, `kb_files`, `kb_chunks` with f32 BLOB embeddings, `kb_fts`):
  own recursive walker (hidden, `node_modules`/`target`/`Library`/… and `.app` skipped, 20k files max,
  photos only on macOS), `chunk` (~2,400 chars, 350 overlap, page from `[Page/Slide/Sheet N]`), changed files
  by mtime+size, `kb://progress`, embeddings after reading (only if the model is downloaded), hybrid
  `search` = FTS5 BM25 + cosine → RRF (k 60). Rescan at launch (+30 s) and every 15 min (**instead of a
  `notify` watcher**: simpler and no permissions prompts; revisit if users want instant updates).
  `settings.kbEnabled` is the module toggle.
- Tool `search_my_files` (`tools/mod.rs`, sources are `file://…#page=N`, marked read), forced first search
  when `router::wants_files` ("my lease/notes/files…"); `AppState.app` (OnceLock) gives turns an AppHandle.
- UI: `KnowledgeTab.tsx`, composer "My files" pill (only when the KB has passages), `Reader.tsx` +
  `lib/reader.ts` (`fileSource`, `locate`), file sources and sent-file chips open the reader.
- Left in Phase 4: item 8, the instant-answer cache.

1. **Attachments in local chats** (`src-tauri/src/files/{mod,pdf,office,text}.rs`): pick/drop/paste →
   `file_ingest(path)` → `{name, kind, pages, text, truncated}`. PDF via `pdf-extract`, DOCX/PPTX/XLSX via
   `zip` + `quick-xml` (document.xml / slide*.xml / sharedStrings.xml), TXT/MD/CSV/JSON/code as UTF-8, HTML via
   `dom_smoothie`. Caps 25 MB / 200 pages. Short files go in whole; long ones are chunked and the best passages
   chosen with `tools::fetch::relevant_passages` (later embeddings). Attachment text (capped) is stored in the
   message JSON so reloads and edits work. Reuse the chip UI from `src/components/chat/Attachments.tsx`.
2. **OCR** (`files/ocr.rs`): Apple Vision `VNRecognizeTextRequest` (`objc2-vision`), runs on the Neural
   Engine; scanned PDFs rendered with PDFKit (`objc2-pdf-kit`) then OCR'd. Linux stub: "OCR needs macOS".
3. **Embeddings engine**: embedding-mode llama-server (`--embedding --pooling mean`, catalog helper
   `nomic-embed-text-v1.5`, ~0.15 GB, role `embed`) via `Extras`/`Engine`, on demand, stopped after 5 idle
   minutes; `embed(texts)` batches `/v1/embeddings`. First KB use offers the download.
4. **Knowledge base** (`kb.rs`, DB schema **v4**): `kb_sources` (path, kind, last scan), `kb_chunks` (text,
   file, page, heading, f32 embedding BLOB) + FTS5 table. Indexer: walk folders (skip hidden, >25 MB, binary),
   ~700-token chunks with ~15% overlap, embed in batches, `kb://progress` events; `notify` watcher re-indexes
   changed files. Search = BM25 (FTS5) + cosine, merged with reciprocal-rank fusion (k = 60).
5. **Tool `search_my_files`**: offered when the KB has content and the composer's "My files" toggle is on;
   results become numbered sources (file + page); questions naming "my notes/files/docs" force a first search.
6. **Reader view** (`src/components/reader/Reader.tsx`): side panel with extracted text, cited passage
   highlighted and scrolled into view; source cards open it (or the file in Finder).
7. **Settings → Knowledge base**: add/remove folders, file counts, index status, re-index, storage used.
8. **Instant-answer cache** (moved from Phase 2): query embedding + mode; cosine ≥ 0.97 and not
   time-sensitive → cached answer marked "from cache" with a re-ask button.
Verify: fixtures in `src-tauri/tests/fixtures/` (PDF, DOCX, PPTX, XLSX, CSV, a scanned PDF), unit tests
(parsers, chunker, RRF, cosine, migration v4), real-engine embeddings test (cache nomic-embed in
`mac-engine.yml`), screenshots (attach, KB tab, reader), owner tests OCR and a real folder.

### Phase 5 — Documents (made on the Mac) ⏳
Goal: real PDF / PPTX / DOCX / shareable HTML files made locally, looking professionally designed. (The
cloud already makes documents through the byte-ai API — keep both; local works offline and on any Mac.)
1. **DocSpec**: the model writes JSON — title, meta, sections of heading / paragraphs / bullets / table /
   chart / callout / image / quote, and sources — via a constrained prompt + JSON repair. Plan-first like the
   cloud: show the outline for approval before writing.
2. **Renderers** (`src/lib/docs/`): HTML/CSS templates (themes: neon, clean, paper, academic; cover, table
   of contents, running headers, page numbers, charts drawn with Chart.js to PNG, sources appendix). **PDF**
   from that HTML with WKWebView `createPDF` (`objc2-web-kit`, hidden webview); the same HTML is the
   shareable page and the print view. **PPTX** with `pptxgenjs` (themed masters, title/agenda/section/bullets
   auto-split, native charts, speaker notes, sources slide). **DOCX** with `docx` (styles, headings, lists,
   tables, links, TOC field).
3. Preview panel (split view) → Save as… (default `~/Documents/BYTE`), print, share sheet
   (`NSSharingServicePicker`), copy as Markdown / plain / RTF / HTML.
4. **Templates library**: cover letter, resume, email, business plan, lesson plan, agenda, blog post, speech,
   poem, lyrics… plus user-added templates; `/report /deck /doc /brief /compare` commands.
5. **Edit existing files**: open a PPTX/DOCX, change it by instruction, save a new version.
6. **Visuals**: LaTeX math (KaTeX) and diagrams from text (Mermaid) in chat and documents; infographics and
   posters (HTML → PNG/PDF); charts on demand.
Verify: renderer smoke tests (vitest: DocSpec → files open, page counts), JSON-repair tests, screenshots,
owner opens the files in Preview / Keynote / Pages / Word.

### Phase 6 — Research+ ⏳
Goal: deeper, more trustworthy answers.
1. **Deep / Extended pipelines** (`research.rs`): plan 3–6 sub-questions → parallel searches → fetch up to
   15 (Deep) / 30 (Extended) pages, 6 at a time → embedding-rank passages → cited report; Extended adds a gap
   review, follow-up searches and a rewrite into a sectioned report. Progress steps shown live; cancel anytime.
2. **Answer quality**: self-check pass against sources, best-of-3 drafts for hard questions, confidence labels
   (verified / likely / unsure), clarifying questions before guessing; **quote finder** (exact supporting
   sentence per claim).
3. **Academic search**: arXiv, Semantic Scholar, PubMed, Crossref / OpenAlex → read PDFs → literature
   review; citation styles APA / MLA / Chicago / Harvard / IEEE + BibTeX export.
4. **Fact-check mode** (claims → sources for / against → confidence), **compare & decide** (weighted table
   with sliders), review summarizer, price compare & deal finder.
5. **Trip planner** (itinerary, budget, packing → PDF + Calendar), **local lookup** (CoreLocation +
   Open-Meteo weather + OpenStreetMap places, no keys).
6. **YouTube**: transcripts (innertube captions; fallback yt-dlp + whisper), summaries with timestamps, Q&A,
   video-to-slides (ffmpeg as an optional download).
7. **Web agent**: hidden WKWebView browser that can browse and click, fill forms (always stops for approval
   before submitting), download files into a folder, save full-page screenshots/archives.
8. Game guides with spoiler-free hints.
Verify: pipeline unit tests with recorded pages, citation formatter tests, real-engine test of a Deep run with
a small model, owner checks answer quality on real questions.

### Phase 7 — Writing & learning ⏳
1. **Writing studio**: split editor; per-paragraph rewrite / expand / shorten / change tone / fix grammar.
2. **Long-form writer**: outline → chapters with a consistent style; **style cloning** from 3–5 samples;
   poetry / lyrics / speeches with meter and rhyme controls.
3. **Flashcards** with SM-2 spaced repetition (DB tables `flashcards`, `reviews`) + Anki `.apkg` export;
   **quiz / exam generator** with grading; **tutor mode** (Socratic, adapts to level); step-by-step math
   solver verified by the calculator.
4. **Instant translate** (text, documents, web pages, selection hotkey).
5. **Job search tracker** (applications, deadlines, company research, interview prep).
6. **Custom assistants and skills**: named assistants with their own instructions/tools, saved prompts as
   `/commands` (cloud ones already work), skill packs import/export, chained workflows.
Verify: SM-2 unit tests, Anki export opens in Anki, editor interactions in screenshots.

### Phase 8 — Speed ✅ mostly done (pulled forward)
Done: Speed boost (family drafters and models' own MTP / EAGLE-3 / DSpark heads), repeated-text guessing,
per-Mac auto-tuning (quick / thorough / tune all), speed preference (Faster / Balanced / Smarter),
measured-speed recommendations with a quality floor and Mac-wide calibration, CPU offload for MoE models and
stretch mode, optional bigger GPU share, accuracy-first thinking router, load several models at once and
compare. Left:
1. **Model lab**: add any GGUF (file or Hugging Face URL) → read its header → RAM check → catalog entry with
   its own settings (context, thinking, temperature); side-by-side compare.
2. **Advanced tuning panel**: temperature, context, thinking budget, system prompt editor, live tok/s and
   RAM/GPU meters.
3. **Auto-switch**: low-battery mode (<20% → faster model, shorter thinking); optional small router model.
4. Maybe later: Apple MLX engine (10–20% faster on some models; a big rebuild).

### Phase 9 — Mac control ⏳
Everything asks for permission once, shows what it will do, and can be undone where possible (activity list
with undo; dry-run mode).
1. **AppleScript/JXA library** (`mac/applescript.rs`, vetted scripts only — the model never writes raw
   scripts): Notes, Reminders, Calendar (EventKit via `objc2-event-kit`), Mail (read, triage, drafts in the
   user's tone, follow-up tracker), Messages (draft; sending needs an explicit click), Music, Safari (current
   tab, open URL), system toggles (dark mode, volume, Do Not Disturb via Shortcuts, Wi-Fi via
   `networksetup`, sleep via `pmset`).
2. **Shortcuts**: run any (`shortcuts run`), list, and create new ones (plist → `shortcuts sign --mode
   anyone` → open for one-tap Add).
3. **Selection tools**: hotkey captures the selected text anywhere (simulated ⌘C with clipboard restore,
   Accessibility permission) → smart reply (3 options → paste), translate, explain; dictate anywhere.
4. **Files**: find (`mdfind` + walkdir), organize (preview → confirm → apply, undo log), convert and compress
   (`sips`, `lopdf`, `textutil`), trash only with confirmation.
5. **Finder Quick Action "Ask BYTE"** + `byte://` deep link; **clipboard history** (200 items, searchable);
   **terminal helper** (proposes a command, explains it, runs only after OK); open-the-setting buttons.
Verify: unit tests for script templating/escaping and the permission gate; owner runs each action on the Mac.

### Phase 10 — Upkeep & automation ⏳
1. **Mac upkeep**: storage analyzer (treemap, duplicates, big/old files, safe-to-delete explanations),
   battery & performance coach, Mac Q&A with open-the-setting buttons, app uninstaller (bundle + leftovers),
   login-items manager, self-diagnostics with fix-it buttons.
2. **Tasks & reminders** (natural language, notifications, optional Apple Reminders sync), **calendar
   awareness**, **daily briefing** (calendar + tasks + weather + followed topics, scheduled).
3. **Scheduler** (`scheduler.rs`, cron): scheduled tasks, news & RSS digest (`feed-rs`), page watchers with
   change / price alerts and wishlists (`similar` diff, JSON-LD prices).
4. **Multi-step agent runs** (plan, progress, checkpoints, asks when stuck) and a visual **automations
   builder** (when trigger → steps).
5. **Trackers**: packages & orders, bills & subscriptions, gifts & events, car & home maintenance.
6. **Connectors** (off until signed in; OAuth in the system browser with a loopback redirect, tokens in the
   Keychain): Google Drive / Gmail / Calendar, Notion, Obsidian vault, Dropbox / OneDrive, Spotify.
7. **Dashboards**: command-deck home dashboard, local usage stats, live hardware panel, research library.
Verify: scheduler/cron and diff unit tests, recorded-feed tests, owner checks notifications and connectors.

### Phase 11 — Input & windows ⏳
1. **Voice**: whisper (sidecar `whisper-cli`, small/base models) push-to-talk / hold Space; hands-free
   conversation; transcripts of dropped or recorded audio with speaker labels (sherpa-onnx diarization).
2. **Vision**: paste/drop images for a vision model (e.g. Qwen2.5-VL 3B + mmproj; swapped in on 16 GB,
   resident on 24 GB+), OCR fallback.
3. **Windows**: Quick Ask floating window (⌥Space), menu-bar mini chat (`tauri-plugin-positioner`), floating
   desktop widget, command palette (⌘K), full keyboard control with customizable shortcuts, notifications.
4. **Wake word "Hey BYTE"** (sherpa-onnx keyword spotter; opt-in; mic indicator).
5. **Notes** (Markdown, folders, tags, save answer as note, feeds the knowledge base), web clipper, mind maps,
   brainstorm board, help center (offline, searchable), example prompts, personality sliders + presets.
6. Optional lifestyle modules (off by default): recipes & meal planning, mindfulness prompts.
Verify: audio pipeline tests with sample files, window behavior checked by the owner.

### Phase 12 — Privacy & polish → public 1.0 ⏳
1. **Privacy**: offline switch (blocks all network, tray indicator), auto-delete chats after N days,
   wipe-all, **Touch ID lock** (LocalAuthentication) with the DB key moved to the Keychain behind it,
   **permission dashboard** (every grant, revoke, full action log), kids mode (no web/files, simple UI, PIN).
2. **Signing without a paid account**: a free self-signed certificate (stored as a GitHub secret) so macOS
   permissions survive updates; README keeps the "Open Anyway" steps.
3. **Looks**: all 20 themes, theme editor with import/export, custom/animated wallpapers, the unique BYTE
   layout with the command-deck home, 60 fps animations (respect reduced motion), optional sounds,
   accessibility pass.
4. **Sharing**: `.byte` chat files, LAN access from another device (password, local network only),
   presenter mode, iCloud Drive backup/restore.
5. **Distribution**: auto-update (Tauri updater + minisign, test and stable channels), Homebrew cask,
   README with screenshots, final logo/icons, version 1.0.0, **public release**.
Verify: owner's full checklist on the M4 Air (in `docs/PROJECT_GUIDE.md` → Verification) and an update from
the last test build to 1.0.

### Cloud track (alongside the phases) — built, redesign next
Done (test.10): API-key connection checked with `/api/auth/me` and kept in the Keychain; chat streaming with
`delta` / `phase` / `sources`, reconnects, message actions (deepen, justify, answer now, feedback, stop),
branching on edit/regenerate, import of cloud chats; photos/files with a reusable library; documents with the
required outline approval, templates, previews, download, revise, render; account data (memories,
knowledge, saved prompts as `/` commands, recipes, personal context, default mode, search, export);
fallback to the local model when the cloud is unreachable; private chats never leave the Mac.
Done (test.11): This Mac · Cloud · Both workspaces, server chat list with open/new/delete, Both answers side
by side with "Keep this one", budgets chip, 429 message, re-read after a dropped stream, invite-only onboarding
path. Next: per-user sign-in once the backend supports it (§3.2 A), then fix any reply-shape mismatches the
owner reports.

### After 1.0 (not scheduled)
Image generation (Core ML Stable Diffusion), bigger-Mac mode for 32 GB+, Windows version (the owner said
"later"). Not wanted: paid Apple Developer ID, iPhone app.
