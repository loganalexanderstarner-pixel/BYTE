# BYTE work log

Every change, newest first: what it did, why, where it lives, and how to undo it.
**Add an entry with every commit** (see CLAUDE.md). If something breaks, find the
change here and revert just that commit (`git revert <hash>`) instead of starting
over from an old copy of the repo.

**CI is manual** (the owner's GitHub Actions minutes are limited): pushing costs
nothing and is the backup, so push every change. Before each push run
`scripts/check-all.sh` (secret scan, typecheck, frontend tests + build, Rust tests,
clippy). Run CI / the Mac engine test / a test build by hand only at milestones
(phases 8, 10, 12) or when the owner asks.

Format: `hash — title` · **Why** · **What** (files) · **Verify** · **Undo**.

---

## 2026-09-28

### (this commit) — CI: the Rust core must compile on Windows
- **Why:** owner decision: Mac phases first, but keep the core portable for the Windows/Linux apps later.
- **What:** `.github/workflows/ci.yml` job `windows` (`windows-latest`, `cargo check --all-targets` with a
  placeholder sidecar). No app changes.
- **Undo:** delete the job.

### (this commit) — Version 1.0.0-test.14 (Phase 4: files, scans, photos, knowledge base, instant answers)
- **What:** `node scripts/bump.mjs 1.0.0-test.14`; CHANGELOG "Unreleased" → test.14. CI and the Mac engine test
  (OCR + embeddings on Metal) green on `4410b53`. Release: `release.yml` with `tag=v1.0.0-test.14`.
- **Undo:** bump back to test.13.

### 33488f7 — Phase 4: instant answers for questions asked before
- **Why:** Phase 4 item 8 (instant-answer cache, moved from Phase 2): speed for repeated questions without
  risking stale answers.
- **What:** `src-tauri/src/answer_cache.rs` (new: `cacheable`, `put`, `find`, `clear`), `db.rs` schema v5,
  `backend.rs` `reuse_earlier_answer`, `commands.rs` `answer_cache_put`/`answer_cache_clear` +
  `ChatRequest.fresh`, `settings.rs` `answer_cache`, `tools::Source` Deserialize, `embed.rs` threshold e2e.
  UI: `store.ts` `rememberAnswer` + `fresh` on Regenerate, `api.ts`, `types.ts`, `KnowledgeTab.tsx` toggle.
- **Verify:** `scripts/check-all.sh`; `cargo test answer_cache`; real model:
  `cargo test e2e_instant_answer_threshold -- --ignored` (with `BYTE_TEST_EMBED_MODEL`).
- **Undo:** `git revert 33488f7`, or turn "Instant answers" off in Settings → Knowledge base.

### 902024a — Phase 4: knowledge base, search by meaning, "My files", reader view
- **Why:** Phase 4 items 3–7 (owner: "keep going with phase 4"): BYTE should answer from the user's own folders
  and cite them.
- **What:**
  - `src-tauri/src/embed.rs` (new): `Embedder` (own `Engine::helper`, `embed.pid`), `embed()` with nomic
    prefixes, normalization, byte packing, cosine; idle stop after 5 min. `engine.rs`: `LaunchOpts.embedding`
    (`--embedding --pooling mean`, FA auto, f16 cache, chat flags removed, no warm-up), `Engine::helper`.
  - `src-tauri/src/kb.rs` (new) + `db.rs` schema v4 (`kb_sources`, `kb_files`, `kb_chunks`, `kb_fts`;
    `Db::conn` now `pub(crate)`): walker, `chunk`, `rrf`, `fts_query`, store/delete files, `index`,
    `embed_pending`, `search`, `schedule` (launch + every 15 min).
  - `commands.rs`: `kb_status`, `kb_add`, `kb_remove`, `kb_reindex`, `kb_search`. `settings.rs`: `kb_enabled`.
    `state.rs`: `embedder`, `app` (OnceLock<AppHandle>). `lib.rs`: modules, reaping/killing `embed.pid`.
  - `tools/mod.rs`: `SEARCH_FILES` tool, `file_url`, `specs(web, memory, files)`, `ToolContext.files`.
    `agent.rs`: `Turn.files`, forced first file search (`router::wants_files`). `backend.rs`: files when
    `kb_enabled` and passages exist. `fetch::worth_reading` only http(s).
  - UI: `settings/KnowledgeTab.tsx`, `reader/Reader.tsx`, `lib/reader.ts` (+ test), store (`kb`,
    `kbProgress`, `refreshKb`, `toggleFiles`, `reader`), Composer "My files" pill + wrapping bar, Activity
    labels + file sources open the reader, sent-file chips open the reader.
  - `mac-engine.yml`: embedding model download + `BYTE_TEST_EMBED_MODEL`; paths. Screenshots `14-*`.
- **Verify:** `scripts/check-all.sh`; `cargo test kb::` and `BYTE_TEST_LLAMA_SERVER=… BYTE_TEST_EMBED_MODEL=…
  cargo test e2e_embeddings -- --ignored`; screenshots 14, 14a–c. On a Mac: Settings → Knowledge base → Add
  folder, download "search by meaning", ask "what does my … say about …".
- **Undo:** `git revert 902024a` (schema v4 tables stay in existing databases, unused; harmless). To switch the
  feature off without reverting: `kbEnabled: false` in settings.

### 9b50c29 — Phase 4: read scanned PDFs and text in photos (Apple Vision)
- **Why:** Phase 4 item 2: scans had "no readable text"; photos of documents only helped models that can see.
- **What:** `src-tauri/src/ocr.rs` (new; `objc2-vision`, `objc2-pdf-kit`, `objc2-app-kit`, `objc2-foundation`
  under the macOS target in `Cargo.toml`): `image_text`, `pdf_text` (PDFKit renders each page → TIFF → Vision,
  max 60 pages). `files.rs`: `has_text_layer`, `scanned_pdf_text`, photos' words read, `Ingested.ocr`,
  `for_model` includes photo text; test helper `tests::test_pdf`. `chat.rs`: note for models that can't see.
  `mac-engine.yml` paths. UI: `LocalFile.ocr`, chip label.
- **Verify:** `scripts/check-all.sh`; Mac: `mac-engine.yml` runs `ocr::tests::e2e_reads_text_from_a_rendered_page`
  (renders a PDF page and reads "Invoice number 48213" back). Attach a scanned PDF in the app.
- **Undo:** `git revert 9b50c29` (or make `ocr::image_text`/`pdf_text` return Err to switch it off).

### 567584e — Phase 4 part 1: files and photos in local chats, models that see images
- **Why:** the owner said "keep going with phase 4": BYTE should read the user's files, and photo buttons should
  appear only when a model that can see is loaded (owner request, 2026-09-28).
- **What:**
  - `src-tauri/src/files.rs` (new): `kind_of`, `ingest` (PDF by page via `pdf-extract`; docx/pptx/xlsx/odt/odp/ods
    via `zip` + `quick-xml`; HTML via `fetch::extract`; text/code; photos → data URL, HEIC/WebP converted and
    photos over 2 MB shrunk to 2048 px with `/usr/bin/sips` on macOS). Caps: 25 MB file, 240k chars, 8 MB photo.
    `for_model` builds `<file name=…>` blocks from `relevant_passages`. Cargo: `pdf-extract`, `quick-xml 0.37`,
    `zip`; dev `lopdf 0.42`.
  - `chat.rs`: `ChatMessage.files` / `images`, `with_files` (called in `backend.rs` before `fit_history`),
    `question_text` (router/agent see only the user's words), image parts in `base_messages`, `IMAGE_TOKENS`.
  - `engine.rs`: `LaunchOpts.mmproj` → `--mmproj`, memory for the adapter, `Endpoint.vision`,
    `EngineStatus::Ready.vision`; the last fallback launch drops the adapter.
  - `models.rs`: `Vision`, `VISION_QUANT`, `download_dir` (adapters in `models/vision/<id>/`), `vision_path`,
    `ModelStatus.vision`; `download_target` handles `"<id>:vision"`. `commands.rs`: `file_ingest`; download and
    delete use `download_dir`. `tune.rs` `launch_opts` sets `mmproj` when downloaded.
  - `scripts/build-catalog.mjs`: `pickVision` (F16, else BF16, else Q8_0; Q8_0 beats a 16-bit file over
    1.5 GB); `--vision` re-checks the catalog in place. `models.json`: 128 models with `vision`.
  - UI: `store.ts` (`pendingFiles`, `attachLocal`, `Message.files`, `toWire` sends files, image-reader download
    restarts the engine when it's the main model's), `Composer.tsx` (paperclip + drop for local chats; photo
    types only when `engine.vision`), `Attachments.tsx` (`LocalFileChips`, `fileDetail`), `MessageView.tsx`,
    `ModelCard.tsx` ("Sees images" tag, `VisionRow`), `types.ts` (`LocalFile`, `FileKind`), `api.ts`.
  - `tools/ui-shots`: `file_ingest` mock, vision state, download mock uses `key`; shots `13-local-files*`.
- **Verify:** `scripts/check-all.sh`; `cargo test files chat::tests::attached image_adapter fallback`;
  screenshots `13-local-files.png`, `13b-local-files-sent.png`, `13c-sees-images.png`. On a Mac: download a
  model's image reader, attach a photo, ask about it; attach a PDF and ask about it.
- **Undo:** `git revert 567584e`. To turn only photos off: have `tune::launch_opts` leave `mmproj` as `None`.

### c0f9649 — Repo public: automatic CI back on; version 1.0.0-test.13
- **Why:** the owner made the repo public (Actions free for public repos).
- **What:** `ci.yml` on every push (skips docs-only), `mac-engine.yml` on engine-related pushes again; CLAUDE.md
  and HANDOFF updated; `node scripts/bump.mjs 1.0.0-test.13`; CHANGELOG "Unreleased" → test.13.
- **Undo:** set the workflows back to `workflow_dispatch` only.

### 93c2a4a — Build on your own Mac: scripts/build-mac.sh
- **Why:** GitHub Actions minutes are at 100%, so release builds can't run on GitHub.
- **What:** `scripts/build-mac.sh`: checks/installs tools (Xcode CLT, Homebrew cmake + node, rustup), builds
  llama-server with Metal (`build-llama-server.sh`), `npm ci`, `tauri build --target aarch64-apple-darwin`,
  verifies the engine is in the bundle and the ad-hoc signature, prints the .dmg path; `--install` copies to
  /Applications and clears quarantine. CLAUDE.md and HANDOFF mention it.
- **Verify:** on an Apple Silicon Mac: `scripts/build-mac.sh --install`. (Syntax-checked here; this container is Linux.)
- **Undo:** delete the script.

### 72ce820 — Catalog: 713 models, community fine-tunes, a details dropdown
- **Why:** the owner asked for ~250 more regular models, the community models left out before (with their
  creator and what they're good for), and a dropdown per model with details and use ideas.
- **What:**
  - `scripts/discover-models.mjs`: looser official filters (older generations, language/domain models, more
    quantizer accounts) → 480 official (was 320); `--community` pass → `scripts/catalog-community.json`: 90
    community models (uncensored capped at 45, one per base model and size; Dolphin; story/role-play tunes;
    repos marked `not-for-all-audiences` and explicit names excluded) and 130 independent models from smaller
    makers (listed as regular).
  - `scripts/enrich-catalog.mjs` (new): `details` per chat model: `about` from the original model's card (never
    a quantizer's card; boilerplate, bios, setup text filtered), `author`, `sourceUrl`, `strengths` 1–5,
    `ideas`, `community`, `caution`. Card starts cached in `scripts/catalog-cards.json` (git-ignored).
  - `scripts/build-catalog.mjs` merges the community list. `src-tauri/catalog/models.json`: 713 models, 1.4 MB.
  - Rust `models.rs`: `ModelDetails`, `CatalogModel::is_community`; `recommend` never picks community models.
    Test `embedded_catalog_is_valid_and_small` now: 600+ chat models, < 2 MB, details for all, authors for 80%+.
  - UI: `ModelCard.tsx` Details dropdown (`ModelDetailsView`), Community badge, "by <creator>";
    `CatalogBrowser.tsx` Community / Stories / Uncensored chips (community models hidden from the main list
    unless those or a search are used); search covers descriptions and creators.
  - `tools/ui-shots`: details come from the current catalog; new shot `07g-model-details.png`.
- **Verify:** `scripts/check-all.sh`; `cargo test community_models_are_never_recommended`; screenshots.
- **Undo:** revert the commit; or rebuild the old catalog with the previous `catalog-discovered.json`.

### 90ded06 — Documents faster, "quit these apps" help, CI manual, work log
- **Why:** the owner found the cloud document maker slow; asked that a model that won't load say which open
  apps use memory and which can be closed; Actions minutes nearly used up; wanted everything documented.
- **What:**
  - `src/components/chat/Attachments.tsx` `cloudImageCached`: one image cache for library photos and document
    pages; a failed load is retried next time. `documents/DocumentsPanel.tsx`: page 1 isn't downloaded twice.
    (The shared cloud HTTP client from 9f854b3 also removes a TLS handshake per page image.)
  - `src-tauri/src/memory.rs` (new): apps by memory (helpers grouped under their `.app`; macOS and BYTE left
    out), `advice()` for error messages, `quit_app()` (normal quit via AppleScript, only apps in the report).
    Commands `memory_report`, `app_quit`. `engine.rs` adds the advice to "didn't start" / "keeps stopping".
    `src/components/MemoryHelper.tsx`: the list with Quit buttons and "Try loading again" under the engine
    error in the chat box.
  - `.github/workflows/ci.yml`: manual only. `scripts/check-all.sh`: every CI check, locally.
  - `docs/WORKLOG.md` (this file).
- **Verify:** `scripts/check-all.sh`; `cargo test memory`; screenshot `tools/ui-shots` → `12-memory-helper.png`.
- **Undo:** revert the commit; the memory helper is self-contained (memory.rs + MemoryHelper.tsx + 2 commands).

### 52dc8fb — Handoff: speed and reliability list
- **What:** `docs/HANDOFF.md` §3.2d, the ordered speed list. Docs only.

### 9f854b3 — Big MoE models load, cloud streams smoothly, CI uses fewer minutes
- **Why:** Qwen3.6 35B-A3B errored on the owner's 16 GB Mac though BYTE said it would run; cloud chat felt slow
  and jerky; Actions minutes.
- **What:**
  - `system.rs` `plan_offload`: 4 GB left for macOS (was 3) and 1 GB GPU headroom (was 0.3) when part of a model
    runs on the CPU. Versions that can't fit are no longer offered (35B-A3B IQ3_XXS on 16 GB).
  - `engine.rs` `Engine::start`: fallback ladder (no helper → half context + ¼ more on the CPU → 4k context with
    every expert / half the layers on the CPU), remembered per session (`Inner.safer`); offloaded models cap
    `ubatch` at 512. `fallback_launches` has unit tests.
  - `cloud/mod.rs`: `http_client()` shared via `AppState.cloud_http` (connection reused); `follow()` reopens a
    stream the server closed mid-answer at once; only empty attempts back off and count toward giving up.
  - `MessageView.tsx` + `lib/throttle.ts`: markdown re-rendered ~12×/s while streaming; `Sidebar.tsx`
    re-renders only when `listSignature` changes.
  - Workflows: CI once per push, mac-engine manual (both later made manual).
- **Verify:** checked locally that Qwen3.6 35B-A3B UD-IQ2_M loads (8 s) and answers with BYTE's flags; all 58
  catalog architectures exist in llama.cpp b11205. Tests: `models::tests::big_moe_models_run_partly_on_the_cpu`,
  `engine::tests::a_model_that_fails_to_load_gets_safer_settings`,
  `cloud::tests::a_stream_closed_after_every_piece_keeps_going_without_waiting`.
- **Undo:** the memory plan is two constants in `system.rs` (`OFFLOAD_OS_RESERVE`, `OFFLOAD_GPU_MARGIN`); the
  ladder is the loop at the end of `Engine::start`.

### 1af0d51 — Version 1.0.0-test.12 (release built: web search fixes)

### 409cd84 — Web search goes through the BYTE cloud's SearXNG first
- **What:** `CloudClient::search` (`GET /api/search`), `tools::search::cloud_search` (relevance filter, rests 5 min
  after a 429, 10 after a 401, 1 after a failure), used first when a key is saved and the chat isn't private.
  Contract: `docs/CLOUD-MODE.md` "Web search".
- **Undo:** pass `cloud: None` in `backend.rs` `local_turn` to go back to keyless search only.

### 7f814f4 — Web search: drop junk results, add Wikipedia and a real weather forecast
- **What:** `search::on_topic` junk filter, Wikipedia search alongside (`search::wikipedia`), DuckDuckGo 2-minute
  rest after a bot check, `tools/weather.rs` (Open-Meteo), `router::weather_place`, reads the latest search's
  results first (`agent::read_top` `from`).

### 2ccb36c — Web search: search first for real questions, read pages, no tool-call leaks
- **What:** `router::wants_web`, `agent::read_top` (BYTE reads pages itself), search budget + repeat guard,
  `ANSWER_NOW` step, `agent::ToolTextFilter`, `fetch::worth_reading`, query cleanup + follow-up topics.
- **Verify:** `agent::tests::e2e_web_quality_report` (real engine + internet): 1/8 → 7/8 usable answers.

### fc0f6f3 — Live web-search quality report test (ignored)

### 9b84a4f — Version 1.0.0-test.11 (release built: workspaces, themes, logo, ModelBackend)

### 14f03ba — ModelBackend: one boundary for answering a turn
- **What:** `backend.rs` (`ModelBackend`, `LocalLlama`, `Cloud`, `with_fallback`); `chat_send` calls
  `backend::answer`.

### c45edf8 — Themes: byte-ai's five-color system, Midnight default, 20 themes
- **What:** `styles/tokens.css` (five tokens per theme, derived with `color-mix`), `design/themes.ts`,
  `styles/tokens.test.ts` (contrast for every theme).

### 1e27bf7 — Logo: BYTE's eight-bit mark, not a bolt
- **What:** `design/Logo.tsx`, app icon + favicon regenerated from `src-tauri/icons/app-icon.svg`.

### 098ecad — Cloud and Both workspaces replace the cloud toggle
- **What:** `settings.workspace`, sidebar switch, cloud chat list/open/new/delete, Both answers side by side
  with "Keep this one", budgets chip, 429 text, re-read after a dropped stream, invite-only onboarding path.

Older history: `CHANGELOG.md` (per test build) and `docs/HANDOFF.md` §8 (every phase).
