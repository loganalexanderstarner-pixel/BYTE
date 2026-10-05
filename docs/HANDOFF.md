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

## 1b. Owner decisions, 2026-10-02 (after 1.0)

Order: **v1.0** (owner's update test + `docs/CHECKLIST-1.0.md`) → **Windows** → **Android** (added 2026-10-04,
`docs/ANDROID.md`) → **Linux** → **v1.1.0 image generation**.

- **Windows first.** The owner has a Windows PC for testing; the plan is a Claude session running on that PC
  (Claude Desktop or `claude remote-control`) to build and run BYTE there. Not a self-hosted GitHub runner (the repo
  is public). Same look as the Mac app on every OS, with native behaviour underneath (owner, 2026-10-04; replaces "feel native" of 2026-09-27).
- **Image generation (v1.1.0):** image models join the model catalog as their own kind. Each card says what the
  model is best at (realism, cartoon, anime, …) and how long one image takes on this hardware. An image model
  loads only when making images, never sits in memory by default. BYTE runs the chat model **or** the image model;
  both at once only when the pair the person chose fits in memory (same fit planning as chat models).
- Uncensored/"obliterated" chat models stay in the catalog; kids mode excludes them (done in v0.12.4).
- Documents are Beta until PowerPoint output is improved.
- **Android app (2026-10-04):** the owner has a Galaxy Z Fold8 Ultra. The catalog for Android covers phones with up to
  16 GB of RAM (the desktop one goes to 128 GB) and is expanded with more models in that range. Plan in `docs/ANDROID.md`.
- **Separate apps per OS.** No linking the PC and the Mac (shared models, chats or engines) unless the owner asks later.

## 2. Where things stand

| Area | State |
|---|---|
| Phases 1–3 (engine, models, chat, web search, agent, calculator, encrypted chats, memory, projects, profiles) | ✅ done |
| Speed work (Phase 8 pulled forward): Speed boost with family drafters **and models' own MTP/EAGLE-3/DSpark heads**, repeated-text guessing (ngram), per-Mac auto-tuning (quick/thorough/tune-all), measured-speed recommendations with a quality floor, CPU offload for MoE (`--n-cpu-moe`) + stretch mode, optional bigger GPU share, accuracy-first thinking router | ✅ done (test.7–test.9); model lab, tuning panel, battery saver in v0.8.0 |
| Cloud mode (chat, streaming, actions, attachments + library, documents with outline approval + templates, account data, `/` prompts, fallback to local) | ✅ built (test.10) — **not yet verified against the real cluster** (see §6) |
| Cloud redesign: This Mac · Cloud · Both workspaces, server chat list with new/delete, Both answers side by side with "Keep this one", budgets chip, 429 message, re-read after a dropped stream, invite-only onboarding path | ✅ built (for test.11) |
| Phases 4–7 (files & knowledge base, local documents, research+, writing/learning) | ✅ done (v0.5.0–v0.7.7) |
| Phases 9–12 (Mac control, automation, voice/windows, privacy & polish) | ⏳ planned — every phase in detail in §8 |
| Windows port (branch `claude/windows-port`; log in `docs/WORKLOG.md`, how to continue in `docs/PORTING-WINDOWS-LINUX.md`) | 🔄 in progress. Checked on a real PC: engines (CUDA and Vulkan builds, chosen by the hardware), GPU detection and model planning for any vendor, chat and tool use through the real engine (about 500 tokens/s on a small model), voice round trip, OCR, secrets in Credential Manager, Recycle Bin, clipboard history, the selection hotkey, and the whole UI worded for a PC (help included). **Not yet:** the installer built by CI and installed on a clean machine (`windows-engine.yml`), Mac-control features (reminders, Mail, Messages, Upkeep, terminal), auto-update, Windows Hello's prompt (availability is checked, the prompt needs a person), and ARM64 (`windows-arm64.yml`, branch `claude/arm64-windows`: cross-built, then run on a real ARM64 runner) |

Builds are GitHub releases tagged by phase, **v0.<phase>.<n>** (latest: v0.5.0; older ones were `v1.0.0-test.1…14`, mapped in `docs/VERSIONS.md`). Each release's description comes from `docs/releases/<tag>.md`. The owner installs them on the M4 Air
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
as the primary source when a key is saved (`docs/CLOUD-MODE.md` "Web search").
**Server-side notes stay server-side:** anything about how the cluster is
configured internally is kept out of this repository, per
`docs/CLUSTER-REQUESTS.md`. The owner's session tracks those privately.

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
- **Item 8 done: instant answers** (`answer_cache.rs`, DB schema **v5** `answer_cache`): first standalone
  questions only (`cacheable`: no files, 8–600 chars, not `router::needs_fresh_info`), same mode, < 7 days,
  cosine ≥ 0.97 (measured with nomic: rewordings 0.995+, different questions ~0.77, test
  `embed::tests::e2e_instant_answer_threshold`). Lookup in `backend::reuse_earlier_answer` (emits
  Started/Notice/Sources/Content/Done "cache"); the UI stores answers via `answer_cache_put` after a first
  local answer; Regenerate sends `fresh`. `settings.answerCache`, Clear in the Knowledge base tab.
- **Phase 4 is complete** apart from paste-to-attach and local files in Both chats.

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
**Owner decision 2026-09-28: a light local version.** The cloud stays the main document maker (it already
does PDF/PPTX/DOCX/flyers/worksheets with outline approval and templates). Phase 5 adds "Make on this Mac"
inside the same Documents panel (same outline approval UI), for offline use, private chats and people without
a cloud invite: PDF, PPTX, DOCX with a few themes (not the full template library), simple charts, preview,
Save as. Not in scope: template library, editing existing PPTX/DOCX, posters/infographics (the cloud has
those). The plan below is the full version; build only the light subset, then go to Phase 6.

**Built (light version, 2026-09-28):** `src-tauri/src/docs.rs` (outline + section JSON via llama-server
`response_format` json_schema, lenient `clean_blocks`, one retry each, optional web research with numbered
sources, reference file via `files::ingest`, progress `DocEvent`s, stop via `chat_cancel`); commands
`doc_outline`, `doc_write`, `doc_save`. Rendering happens in the UI so it ports to Windows/Linux as is:
`src/lib/docs/` — `spec.ts` (types, 4 document themes), `pdf.ts` (pdfmake; `pdfDefinition` clones the spec
because pdfmake mutates lists), `pptx.ts` (pptxgenjs, `planSlides`: ≤6 bullets / ~550 chars per slide, one
table/chart per slide, native charts), `docx.ts` (docx, TOC field), `charts.ts` (Chart.js → PNG), `render.ts`
(lazy imports). UI `components/documents/LocalDocs.tsx` (form → outline editor → progress → preview → Save as
any format), Documents panel "This Mac / BYTE Cloud" switch, 📄 button always shown. Verified: real-engine
e2e with Qwen3-0.6B (outline + section JSON), vitest renderers, sample files opened with python-docx /
python-pptx / PyMuPDF (LibreOffice is broken in this container), screenshots `15-*`.

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

### Phase 6 — Research+ ✅ (v0.6.0–v0.6.7)
Goal: deeper, more trustworthy answers.
**Done in v0.6.0:** item 1 (`research.rs`: plan → parallel searches → 12/24 pages → passages ranked by meaning
or words, ≤3 per source, ~half the context → Extended gap review → report rules), confidence labels, and item 3
without PDF reading (`tools/academic.rs`: Crossref + Europe PMC + arXiv; OpenAlex and Semantic Scholar refuse
keyless requests from shared networks; `academic_search` tool in Deep/Extended; `Source.meta`;
`src/lib/citations.ts` + Cite menu). **Done in v0.6.1:** item 4's fact-check (`factcheck.rs`, verdict table with
quotes = the quote finder, `markdown.ts` `markVerdicts`, shield button → `ChatRequest.task = factCheck`) and compare
& decide (`decide.rs`: options read from the question, criteria + scores via JSON, `ChatEvent::Decision`,
`components/chat/Decision.tsx` with weight sliders, `lib/decide.ts`). **Done in v0.6.2:** item 5 (`tools/places.rs`:
OpenStreetMap Overpass with mirrors, category map, opening-hours "open now", `find_places` tool + forced lookup via
`router::places_request`, `settings.homePlace`, `Places.tsx`; `trip.rs`: ask → weather (forecast or last year via the
Open-Meteo archive) → research + sights → `TripPlan` JSON → `Trip.tsx`, `lib/trip.ts` (PDF via DocSpec), `lib/ics.ts`,
`calendar_open` command). CoreLocation left for Phase 12 (needs a signed app). **Done in v0.6.3 (owner: "unthrottle… for free", nothing that
needs them or the cluster):** per-engine pacing (only DuckDuckGo/Bing scraping), `tools/cache.rs` memory caches
(searches 1 h, papers 24 h, places 1 h), the user's search overlaps the planning call, `settings.researchDepth`
(Normal/More/Max → `research::depth_at`, `batch`, `scale_pages`). Free keys (OpenAlex/Semantic Scholar) and self-hosted
Open-Meteo/Overpass on the cluster were offered and declined for now. **Done in v0.6.4 (owner requests):** Web
Off/Auto/Always (`settings.webMode` beside `webSearch`; `router::wants_web_in`, `Turn.web_always`, `lib/web.ts`) and the
Kitchen module brought forward from K (`kitchen.rs`: `kitchen_ask`, chef rules, JSON-LD recipe reading via
`fetch::fetch_html` + `recipe_ld`, recipe/ideas/meal-plan cards, recipe box = DB v6 `recipes`; UI
`components/kitchen/*`, `lib/recipe.ts`; `settings.kitchenEnabled`). Generated food photos wait for image generation.
**Done in v0.6.5:** item 6 without video-to-slides (`youtube.rs`: `video_id`, innertube player (ANDROID → IOS → WEB
clients), `pick_track`, `parse_timedtext` (format 3 with `<s>`, legacy `<text>`), transcripts cached 24 h, summary
(one pass or map-reduce by part) → `ChatEvent::Video`, Q&A via `research::rank_texts` over 90 s blocks, follow-ups
find the link in the last 3 questions; UI `VideoCard.tsx`, `lib/video.ts`). No-captions fallback (yt-dlp + whisper)
waits for Phase 11's voice model. **Done in v0.6.6:** item 7, the web agent (`web_agent/`): a hidden, private
Tauri window (`browser.rs`, label `byte-agent`, incognito, Safari UA) driven by `bridge.js` (injected on every page:
numbered snapshot, click, type, choose incl. radio groups, scroll, formInfo). Replies come back as a navigation to
`byteagent://r/<id>?d=<json>` that `on_navigation` catches and cancels (no IPC for web pages; works on WebKit,
WebKitGTK and WebView2). `Session` runs the tools (`open_url`, `click`, `type_text`, `choose_option`,
`look_at_page`, `scroll_page`, `go_back`, `download_file`, `save_page`) inside `agent::run` (`Turn.agent`,
`Task::Browse`, `web_agent::wants_web_agent`); guards in Rust: approval cards (`ChatEvent::Approval`, command
`agent_approve`, 10 min → deny, a Deny ends browsing), no private fields, public hosts only, 25 steps, 200 MB
downloads to Downloads/BYTE, older page views compacted. Page capture on macOS: `capture_mac.rs` (WKWebView PDF,
snapshot → PNG, web archive), text elsewhere. UI: `AgentCards.tsx` (approval card, saved-file chips, Show browser),
Agent pill, Settings toggle. Tested for real under Xvfb (`browser::e2e`, `agent::tests::e2e_web_agent`).
**Done in v0.6.7 (Phase 6 finished):** `reviews.rs` (`wants_reviews`, `subject` (shared cue/filler stripper),
`rating_on_page` from AggregateRating/Review JSON-LD, `review_texts`, `parse_reviews` → `ChatEvent::Reviews`),
`prices.rs` (`wants_prices`, `offers_on_page` from Product/Offer/AggregateOffer JSON-LD or `product:price` meta,
`matches_product`, `tidy` → `ChatEvent::Prices`; no model involved in prices), `games.rs` (`wants_game_help`,
`parse_hints` → `ChatEvent::Hints`; the model only sees the first hint), `selfcheck.rs` (`cited_sentences`,
`source_blocks`, `check` → `ChatEvent::SelfCheck`, after Deep/Extended/fact-check answers), `drafts.rs`
(`is_reasoning`, `final_answer`, `pick` majority, `reconcile_prompt`; Deep/Extended, research skipped for such
questions). Shared: `fetch::{json_ld, ld_is, meta_content}`, `research::read_html_pages`. `Turn.modules`
(`agent::Modules`) from five settings (all on). UI `ShopCards.tsx`, `lib/cards.ts`. Video-to-slides moved to
Phase 11. **v0.6.8 (owner request):** recipe measures (`settings.measureUnits` us|metric, `units.rs` +
`lib/units.ts`: mL/g → tsp/tbsp/cup/oz/lb with baking densities, °C ↔ °F in step text, metric amount from the note;
`kitchen::chef(metric)`; US/Metric toggle on `RecipeCard`). **Next:** Phase 7 (writing & learning) as v0.7.0.
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
6. **YouTube**: transcripts (innertube captions; fallback yt-dlp + whisper), summaries with timestamps, Q&A.
   (Video-to-slides moved to Phase 11, with the ffmpeg/whisper download.)
7. **Web agent**: hidden WKWebView browser that can browse and click, fill forms (always stops for approval
   before submitting), download files into a folder, save full-page screenshots/archives.
8. Game guides with spoiler-free hints.
Verify: pipeline unit tests with recorded pages, citation formatter tests, real-engine test of a Deep run with
a small model, owner checks answer quality on real questions.

### Phase 7 — Writing & learning ⏳ (v0.7.0: study tools done)
**Done in v0.7.0:** item 3 without the step-by-step math solver: `study.rs` (SM-2 `review`, `study_ask` routing,
flashcards/quiz JSON + `parse_cards`/`parse_quiz`, DB v7 `decks`/`cards`/`card_reviews`, `study_queue`, `card_review`,
`anki_text` export, `TUTOR_RULES` + `TUTOR_NUDGE` + structured `tutor_reply` for `Task::Tutor`, `reply_for` canned replies);
UI `components/study/{StudyCards,StudyPanel}.tsx`, `lib/study.ts`, 🎓 top-bar button, Tutor pill; `settings.studyEnabled`.
Anki export is a tab-separated import file (Anki 2.1.55+ headers), not .apkg.
**Done in v0.7.1 (owner question: "do the cool features work in cloud mode?"):** `agent::specialist` (the card flows,
split out of `run`), `agent::prepare` + `Prepared` + `cloud_message`; `backend::Setup` (one place that builds a local
`Turn`), `prepare_for_cloud` in `backend::answer`: Cloud (not Both) with a local model loaded runs the card flow here,
sends the question plus notes to the cloud, and re-sends the card's sources after; study cards get `study::reply_for`
with no cloud call. Tutor hints are checked with `solve_linear`/`states_value`. **Done in v0.7.2:** tested the card flows on Gemma 3, Llama 3.2 and DeepSeek-R1 (not just Qwen) and fixed what
broke (`chat::plain_body` fallback, reasoning room, `Quirks`); `serde_json` `preserve_order`; cards in Cloud mode
with no local model (`cloud/json.rs`); `docs/CLUSTER-REQUESTS.md`. **Done in v0.7.3:** `quality.rs` (card checkers + one repair,
small models only) and `looker.rs` (photo helper). **Done in v0.7.4:** `writing.rs` + `WritingPanel` (writing studio), the "a better model fits" hint
(`models::better_model`). **Done in v0.7.5:** `translate.rs` (chat + studio), `jobs.rs` (DB v8, JobsPanel). **Done in v0.7.6:** `assistants.rs` (DB v9) +
AssistantsPanel. **Done in v0.7.7:** long-form writer + "Write like me" (`writing.rs`, `Longform.tsx`). **Phase 7 is complete.**
**Done in v0.8.0:** `gguf.rs` + `lab.rs` (model lab, `<data>/added_models.json`, `CatalogStore::set_added`), per-model
`settings.model_overrides` (`ModelOverride::apply`) + `TuningPanel`, `battery_saver` (`system::battery`), `commands::engine_live`
(with the loaded model's recommended sampling), assistants in Cloud mode. **Phase 8 is complete.**
**Next:** **Phase 10 is complete** (v0.10.0–v0.10.6, all released). Phase 11 (input & windows) has started: v0.11.0
Quick Ask + menu bar + ⌘K palette + custom shortcuts, v0.11.1 voice input and v0.11.2 speaker labels + videos without
captions v0.11.3 spoken answers + hands-free + "Hey BYTE" and v0.11.4 BYTE's own natural voices (Kokoro) and v0.11.5 the voice catalog + human speech and v0.11.6 notes, web clipper and mind maps and v0.11.7 personality, help, examples and the board are done:
**Phase 11 is complete.** Phase 12 (privacy & polish) has started: v0.12.0 offline switch, Touch ID lock and the Privacy tab and v0.12.1 kids mode, encrypted backups, auto-delete and erase and v0.12.2 theme editor, motion and accessibility and v0.12.3 signed one-click updates + Homebrew + README and v0.12.4 (1.0 polish; first update installed through the updater) are done; next v1.0.0 once the owner's Mac checklist passes. Releases: dispatch `release.yml` only for the branch head. When the commit being released is no longer the newest on the branch, GitHub refuses to create the release ("Resource not accessible by integration"; v0.10.0, v0.11.2 and v0.12.1 all failed this way, with the .dmg still saved as a run artifact).
(see the Phase 11 section). Way of working
(owner, 2026-09-29): build the next item during every wait; run long model tests from a copied test binary.
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

### Phase 8 — Speed ✅ done (v0.8.0)
Done: Speed boost (family drafters and models' own MTP / EAGLE-3 / DSpark heads), repeated-text guessing,
per-Mac auto-tuning (quick / thorough / tune all), speed preference (Faster / Balanced / Smarter),
measured-speed recommendations with a quality floor and Mac-wide calibration, CPU offload for MoE models and
stretch mode, optional bigger GPU share, accuracy-first thinking router, load several models at once and
compare. Done in v0.8.0:
1. **Model lab**: add any GGUF (file or Hugging Face URL) → read its header → RAM check → catalog entry with
   its own settings (context, thinking, temperature); side-by-side compare.
2. **Advanced tuning panel**: temperature, context, thinking budget, system prompt editor, live tok/s and
   RAM/GPU meters.
3. **Auto-switch**: battery saver (<20% unplugged → Auto mode, short thinking). A small router model was not needed
   (the rule-based router is accurate enough).
4. Maybe later: Apple MLX engine (10–20% faster on some models; a big rebuild).

### Phase 9 — Mac control ✅ done (v0.9.0–v0.9.3)
**Done in v0.9.0:** `macctl.rs`: fixed AppleScripts that take the user's words only as `argv` (never pasted into
code), rules first (`plan`, `when_in` times, `reminder_title`) and `complete_json` for details (only the title for
notes; list/calendar names only when said next to the noun); approval card (`web_agent::ask`, action "mac") for
anything lasting; `ChatEvent::MacDone` card with Undo (`mac_undo`, in-memory tokens); `settings.mac_control`.
Reminders, Calendar (AppleScript; repeating events don't expand — EventKit later), Notes, Music, Safari tab, dark
mode, volume/mute, Wi-Fi (`networksetup`), display sleep (`pmset`), System Settings panes, Shortcuts list/run.
Tests: unit + fake runner flow; real models (Qwen3 0.6B, Gemma 3 1B, Llama 3.2 1B) for details; on macOS CI
`e2e_scripts_compile_and_run` compiles every script with `osacompile`.
**Done in v0.9.1:** Mail (`MAIL_LIST` read/summarize, `MAIL_DRAFT` opens a filled-in compose window, never sends;
Undo deletes it; replies take the address, "Re:" subject and text from the sender's latest email), Messages
(`MESSAGE_DRAFT`: clipboard + `sms:` link, the user presses Send), Contacts lookup (`find_person`: exact name wins,
asks "Which Sam?"), `without_placeholders` for model drafts.
**Done in v0.9.2:** `selection.rs` (⌥⌘B via tauri-plugin-global-shortcut; front app + System Events ⌘C, clipboard
restored; `selection_paste` activates the app and ⌘V; event `selection://captured` opens the studio with `app`),
`clipboard.rs` (NSPasteboard via objc2-app-kit; opt-in history in DB v10 `clip_history`, 200 items, concealed /
transient types and `looks_secret` skipped, BYTE's own writes ignored), writing `Reply` (own system prompt `HELPER`,
written whole and checked with `echoes`, retried twice) and `Explain`. **Done in v0.9.3:** `filectl.rs` (Spotlight find via `mdfind`,
`tidy_plan`/`apply_moves` with `Undo::Moves`, `sips` photo conversion with `Undo::Created`, Finder selection read +
`files::ingest`), `terminal.rs` (`recipe` known-good commands for common asks — digits only from the message;
`propose` via `complete_json` for bigger models only; `refused` hard block list; approval; `zsh -c`), `macctl::Undo` enum + `keep_undo`/`ask_ok`, `scripts_avoid_applescript_keywords_as_variables`.
**Phase 9 is complete.** Moved: the `byte://` link and a Finder "Ask BYTE" Quick Action to Phase 11 (with Quick Ask);
creating Shortcuts to Phase 10 (automations).
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

### Phase 10 — Upkeep & automation ✅
**Done in v0.10.0:** `upkeep.rs` (+ `upkeep_tests.rs`, `UpkeepCards.tsx`, `lib/upkeep.ts`): storage scan (home
folder, not ~/Library apart from caches; files inside packages/hidden folders never offered), duplicates (size →
first MB → full SHA-256), old installers, developer caches; Trash via Finder (`TRASH`/`PUT_BACK` scripts, Undo);
the Tauri commands take ids from the latest scan, never paths. Health (`df`, `memory_pressure`, `top`, `pmset`,
`system_profiler`, `sysctl`, `tmutil`, `fdesetup`, `socketfilterfw`), Quit for listed apps only; uninstaller
(bundle id, Library leftovers, refuses com.apple.* and BYTE); login items (System Events, Undo re-adds).
Setting `macUpkeep`.
**Done in v0.10.1:** DB v11 (`tasks`, `schedules`, `runs`); `scheduler.rs` (specs "weekdays 07:30" etc., `next_after`
with DST handling, `spec_in` for words, a 30 s loop from `lib.rs`, unattended runs through `backend::answer` with a
collecting Channel, saved as chats, notifications via `tauri-plugin-notification`, `schedules://ran`); `tasks.rs`
(to-dos, reminders, repeats, chat routing, scheduled questions after an approval card; "remind me" goes to Apple
Reminders when Mac control is on); `briefing.rs` (composed by BYTE, not a model: Gemma 1B invented to-dos and news
when a model wrote it; sent as BYTE's own reply like flashcards). UI `TasksPanel.tsx`, `lib/tasks.ts`; settings
`tasksEnabled`, `briefingTopics`. Schedules only run while BYTE is open (open at login came in v0.10.3).
**Done in v0.10.2:** DB v12 (`feeds`, `feed_items`, `watchers`, `watch_events`). `feeds.rs`: RSS 2.0/RSS 1.0/Atom via
`quick-xml` (no new crate), feed discovery from a page's `<link rel=alternate>` then common paths, items stored once
(`UNIQUE(feed_id, guid)`, 300 kept per feed, only the newest 5 count as new when a feed is first followed), a digest
composed by BYTE (kind `feeds_digest`, sent as BYTE's own reply like the briefing; a scheduled digest is just a
scheduled question). `watchers.rs`: "change" watchers compare the page's content lines (lines under 25 characters
ignored, so menus, dates and counters don't count; SHA-256 of the lines) and describe what's new; "price" watchers
read `prices::offers_on_page` (JSON-LD/Open Graph), then `itemprop="price"` microdata, then Amazon's price-to-pay
element (tested on a snippet only: Amazon serves CI a Captcha). Checked from the scheduler tick (3 per 30 s tick,
every 1/6/24 h), notification + `watchers://changed`. Chat routing after tasks; adding a watcher needs the approval
card. `tools::fetch::fetch_raw` (feeds; also accepts XML). Setting `watchEnabled`, `Modules.watch`. UI:
`WatchSection.tsx` in the ✅ panel, `lib/watch.ts`. Live-checked with `cargo test e2e_real -- --ignored` (HN, Rust
blog, The Verge via discovery, BBC, GitHub blog). Fixed: `briefing::write` used `web_mode != "off"` (never true)
instead of `web_search`.
**Done in v0.10.3:** DB v13 (`automations`; `runs` gains `automation_id` + `steps` JSON). `automations.rs`: trigger
("manual", "launch" or a scheduler spec) + up to 8 `Step`s (Ask through `scheduler::ask_unattended`, Briefing, Notify,
AddTask, SaveFile into ~/Documents/BYTE/Automations with `file_stem`, Shortcut via `shortcuts run -i/-o`); `execute`
hands each step's text on (`ask_prompt` adds it only when the step says "it/that/…" or `{previous}`), stops at the
first failure and resumes "from step N" with the saved results; runs in the background, `automations://progress`,
one chat per run, notification. Requests are read by rules (`plan`: trigger via `spec_in` / "when BYTE opens", split
on then/;/"and save|send|add|run…", every Ask part must start with an instruction verb), routed first in
`agent::specialist`, approval card before saving/running; `ChatEvent::AutomationRun` + `RunCard.tsx`.
`web_agent::UNATTENDED` (task-local set by `ask_unattended`) declines approval cards at once. `shortcut_make.rs`:
binary plist (URL + Open URLs actions, `plist` crate) → `shortcuts sign --mode anyone` → `open`; fallback shows the
link. `background.rs`: `tauri-plugin-deep-link` (`byte://run/<id>?key=` with a 64-hex per-automation key,
`byte://ask?q=` fills the composer only), `tauri-plugin-autostart` LaunchAgent with `--background` (main window is
`visible: false` and shown in setup unless started that way), keep running (CloseRequested hides on macOS,
`RunEvent::Reopen` shows). Settings `automationsEnabled`, `openAtLogin`, `keepRunning`. UI `AutomationsSection.tsx`
(builder), `lib/automations.ts`. Screenshots `28-*`.
**Done in v0.10.4:** DB v14 (`trackers`: kind, next date, done, the record as JSON). `trackers.rs` (+ `trackers_tests.rs`):
one `Tracker` for four kinds (package, bill, event, upkeep); `date_in` (month names, 5/14, weekdays, "the 12th");
`next_on_or_after` (month ends kept: the 31st lands on Feb 28, then back to the 31st); `carrier_of` (UPS 1Z, USPS
91–95 / ..US, FedEx 12/15/96…, DHL 10 digits / JJD, Amazon TBA) with the carrier's tracking link, no status
scraping; `money_in`/`money_text`/`per_month`; `take_due` from the scheduler tick sends one notification per date
(bills 3 days, events 14, upkeep 7, packages on the day) and moves passed bills and yearly dates on; chat `ask` by
rules, routed after automations; lists composed by BYTE (canned kind `trackers_list`); "gift ideas for X" gives the
model what's saved. UI `TrackersSection.tsx` (tabs, add rows, totals), `lib/trackers.ts`. Setting `trackersEnabled`.
Screenshots `29-*`.
**Done in v0.10.5:** `connectors/` (owner decision: only connectors that need no app registration now; Google,
Dropbox, OneDrive, Spotify later): `Secrets` (Keychain service `com.loganstarner.byte.connectors`, accounts `notion` and
`calendars`; non-macOS refuses), settings `connectorsEnabled`, `obsidianVault`, `notionParent`. `obsidian.rs` (walks the
vault skipping dot folders, term-scored search with snippets, `write` into `<vault>/BYTE/` never overwriting, Undo via
`macctl::Undo::Created`, `obsidian://open` links). `notion.rs` (Notion-Version 2022-06-28; `me`, `search`, block text,
`create` under the parent page; `page_id` from links; wiremock tests). `ics.rs` (unfolding, DTSTART with UTC/date/TZID
read as local, RRULE DAILY/WEEKLY BYDAY/MONTHLY/YEARLY with INTERVAL/COUNT/UNTIL, EXDATE, cancelled skipped; 30-min
cache; webcal→https). Chat routing in `agent::specialist` after automations (Obsidian/Notion by name; calendar links
only when the Mac Calendar isn't used). `briefing::merge_events` adds link events. UI `ConnectorsTab.tsx`
(Settings → Connectors), screenshot `30-connectors`.
**Done in v0.10.6:** `dashboard.rs`: `summary` (one cheap read over tasks, trackers, automations, watchers, feeds, research
count) for the home tiles; `today_events` (Mac Calendar when Mac control is on + calendar links, 5-min cache); `usage`
(SQL with `json_extract` over message data: counts, average tokens/s, modes, 14-day histogram); `research` (chats with
cited answers, LIKE search). UI `Deck.tsx` in `EmptyState` (tiles from `lib/dashboard.ts`, hidden when empty; a click
asks in chat), the research library modal, Settings → About "Your usage". The live hardware meters were already in
Settings → Engine (`engine_live`). Screenshots `31-*`. **Phase 10 complete.**
**Split of the rest:** v0.10.1 tasks & reminders + scheduler (`scheduler.rs`) + daily briefing;
v0.10.2 news/RSS digest + page watchers (price alerts) ✅; v0.10.3 ✅ multi-step runs + automations builder + creating
Shortcuts; v0.10.4 trackers; v0.10.5 connectors; v0.10.6 dashboards.
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

### Phase 11 — Input & windows 🔄
**Plan:** v0.11.0 windows and keys · v0.11.1 voice (whisper-cli sidecar, push-to-talk, dropped audio) · v0.11.2
diarized transcripts, videos without captions, video-to-slides (ffmpeg download) · v0.11.3 hands-free (spoken replies
via macOS `say`) + wake word · v0.11.4 notes, web clipper, mind maps · v0.11.5 board, help center, example prompts,
personality sliders, mindfulness (off by default). Vision (item 2) was done in Phase 4, recipes in Phase 6.
**Done in v0.11.0:** `quick.rs`: Quick Ask window (label `quick`; `main.tsx`/`App.tsx` `windowKind()` renders
`app/QuickAsk.tsx`, which reuses `ChatView` + `Composer` and the same store, so chats are ordinary saved chats;
`quick://saved` → main `addNewChats`, "Open in BYTE" → `quick_open` → main `openFresh`), hidden on blur and Esc,
placed under the tray icon or high on the pointer's screen; menu-bar icon (template `icons/tray.png`, Ask / Show /
Quit); global shortcuts from settings (`quickAskKeys`, `selectionKeys`; `quick::check` refuses keys without
⌘/⌥/⌃ or a clash; one handler dispatches by shortcut id); ⌘K palette (`components/Palette.tsx`, `lib/palette.ts`
fuzzy ranking) over actions, settings tabs, modes, themes and chats; Settings → About → Keyboard and menu bar
(`KeyboardSection.tsx`, key recorder, `lib/keys.ts`). No positioner plugin was needed (tray click rects).
**Done in v0.11.1:** `voice.rs`: `whisper-cli` as a second sidecar (`scripts/build-whisper.sh`, `WHISPER_TAG`;
Metal on a Mac, `GGML_NATIVE=OFF` so a binary never hits "illegal instruction" on another CPU), speech models
(`voice::MODELS`: large-v3-turbo q5_0, base.en) in `<models>/voice/` through `Downloads` (keys `voice:<id>`),
`voice_status/download/delete/transcribe` (UI WAV, base64) and `file_ingest` routing audio to `voice::ingest`
(`FileKind::Audio`, transcript text; M4A/AAC/AIFF/CAF via `afconvert`). UI: `lib/wav.ts` (downsample, PCM16 WAV),
`lib/recorder.ts` (Web Audio, ScriptProcessor), `MicButton.tsx` (click or hold Space; first press offers the model
download), `VoiceModels.tsx`, Settings → Models → Voice. The webview grants the mic (no permission handler set);
macOS asks once (Info.plist `NSMicrophoneUsageDescription`). Mac test: `voice::tests::e2e_whisper_transcribes`
(base.en + whisper.cpp's JFK sample).
**Done in v0.11.2:** `speakers.rs`: sherpa-onnx v1.13.8 `sherpa-onnx-offline-speaker-diarization` as the
`sherpa-diarize` sidecar (`scripts/build-sherpa.sh`, `SHERPA_TAG`, static onnxruntime; `SHERPA_CMAKE_EXTRA` passes
`FETCHCONTENT_SOURCE_DIR_*` where GitHub archive downloads are blocked, as in the cloud container), models
pyannote-segmentation-3.0 + 3D-Speaker ERes2Net (voxceleb, English) in `<models>/voice/{seg,emb}/` (keys
`voice:seg`/`voice:emb`), threshold 0.9 (on sherpa's 4-speaker sample: 0.9 → 5, 1.0 → 3, so over-splitting is the
safer side); `voice::transcribe_segments` (whisper with timestamps) + `label()` (most overlap per segment, gaps take
the previous speaker, numbered by first appearance). `voice::ingest` uses it when `voiceSpeakers` and the models
are there. `media.rs`: yt-dlp (`yt-dlp_macos`/`_linux`/`.exe` from `releases/latest`, checked against
`SHA2-256SUMS`, `<data>/tools/`), `video_audio` (fixed arguments, id after `--`, `--match-filter duration <= 3h`,
`--print before_dl:` metadata); `youtube::spoken_transcript` is the fallback when there are no captions
(`VideoCard.transcribed`). Video-to-slides (ffmpeg key frames) is still later.
**Done in v0.11.3:** `speech.rs` (`say -v -r -f <tempfile>`, one player, `/bin/kill` to stop, `speech://done`,
`speakable()`, `say -v '?'` voices), store `speakingId`/`talk`/`lastAnswer` + `speak()` (auto-read in `send`'s
finally when `readAloud` or Talk), 🔊 in `MessageView`, Talk loop in `Composer` (`MicButton` `start({auto})` with
`lib/handsfree.ts` `SilenceDetector`, `isStopPhrase`), `wake.rs` (cpal input on a dedicated thread, `Vad`,
`voice::transcribe_short` with `--prompt "Hey BYTE"`, `is_wake`, `quick::show` + `wake://heard` to the Quick Ask
window, `wake_pause` from the UI while recording). Mac test: `e2e_wake_word` (say → afconvert → detect),
`e2e_say_writes_audio`.
**Done in v0.11.4:** `tts.rs`: Kokoro v1.0 via the `sherpa-tts` sidecar (sherpa-onnx `sherpa-onnx-offline-tts`, from
`build-sherpa.sh`), model archive from sherpa-onnx's `tts-models` release into `<models>/voice/kokoro` (`.ready`
marker after unpack), 28 voices (`VOICES`, setting `byteVoice`). Streaming: the store feeds `speech_feed(id, text,
done)` with the answer so far; `tts::feed` makes chunks from new finished sentences in one sequential maker task,
resamples to the device rate and appends to one queue; a single cpal output stream plays it (no gaps); `speech://done`
when generation is done and the queue is empty. Without the voices, `say` is used. Note: our local Linux static build
of sherpa-tts crashes (onnxruntime static `std::regex`); local tests use the official prebuilt binary; macOS builds
with libc++ (checked by the Mac test).
**Done in v0.11.5:** `voices.rs` + `catalog/voices.json` (built by `scripts/build-voices.mjs`, which streams each release
archive once for its SHA-256; cache in `scripts/voices-cache.json`): Kokoro, Piper, Kitten, Supertonic, Pocket via one
sidecar with per-engine args. `byteVoice` = "<package>/<speaker>" (old Kokoro ids map; Kokoro keeps its v0.11.4
folder). Speech shaping in `tts.rs` (`pieces`, `chunks`, `pause_ms`, `chunk_speed`, `shape`); `prompt::SPOKEN` when
`ChatRequest.spoken`; cloud voice `cloud/voice.rs` (cluster request 7; 404 → Mac voice). Wake turns always speak and
then listen 8 s for a follow-up (Composer `followUp`). Sizes measured on Linux x86 with the prebuilt tool: Piper
~170 MB, Kitten nano ~140 MB, Supertonic ~270 MB, Kokoro ~500 MB, Pocket ~570 MB while speaking.
**Done in v0.11.6:** `notes.rs` (Markdown files in Documents/BYTE/Notes, front matter, `resolve` refuses ids outside the
folder, the folder becomes a KB source when the KB is on), `byte://clip` → `notes::clip` (bookmarklet in Settings →
About), `mindmap.rs` (`from_markdown` without a model, `from_model` otherwise) + `MindMapView` radial layout.
"note: …" in chat still goes to Apple Notes (Mac control); BYTE notes come from 📝 / the panel.
**Done in v0.11.7:** personality (`prompt::Personality`; Balanced adds no prompt text), help center (`src/help/*.md`
shared by the UI and `help.rs`, which feeds app-help questions to the model), daily example prompts
(`lib/examples.ts`), brainstorm board (`board.rs`, DB v15). The mindfulness module (optional K) was not built; add it
only if the owner asks.
Later: the Finder "Ask BYTE" Quick Action (a Shortcut or Service that opens `byte://ask`), a floating desktop
widget.
1. **Voice**: whisper (sidecar `whisper-cli`, small/base models) push-to-talk / hold Space; hands-free
   conversation; transcripts of dropped or recorded audio with speaker labels (sherpa-onnx diarization).
1b. **Video-to-slides and videos without captions** (moved from Phase 6): ffmpeg keyframes + whisper transcript.
2. **Vision**: paste/drop images for a vision model (e.g. Qwen2.5-VL 3B + mmproj; swapped in on 16 GB,
   resident on 24 GB+), OCR fallback.
3. **Windows**: Quick Ask floating window (⌥Space), menu-bar mini chat (`tauri-plugin-positioner`), floating
   desktop widget, command palette (⌘K), full keyboard control with customizable shortcuts, notifications.
4. **Wake word "Hey BYTE"** (sherpa-onnx keyword spotter; opt-in; mic indicator).
5. **Notes** (Markdown, folders, tags, save answer as note, feeds the knowledge base), web clipper, mind maps,
   brainstorm board, help center (offline, searchable), example prompts, personality sliders + presets.
6. Optional lifestyle modules (off by default): recipes & meal planning, mindfulness prompts.
Verify: audio pipeline tests with sample files, window behavior checked by the owner.

### Phase 12 — Privacy & polish → public 1.0 🔄
**Done in v0.12.0:** offline switch (`offline.rs`: a connector layer on every internet client, so proxies can't
bypass it; tray tick, top-bar pill, ⌘K), **Touch ID lock** (`lock.rs`: LocalAuthentication, idle timer, data
commands refuse while locked; the DB key stays in `db.key` because a Touch ID–bound Keychain item needs a paid
signature), **Privacy tab** (`privacy.rs`: internet list, Mac permissions with live Microphone/Accessibility
status, the activity log from `actions.jsonl` with search, filters, copy, clear).
**Done in v0.12.1:** kids mode (`kids.rs`), encrypted backups to iCloud Drive with restore (`backup.rs`), auto-delete
old chats, erase everything.
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
