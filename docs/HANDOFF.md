# BYTE — handoff for the next session

**Read this first, then `CLAUDE.md` (rules), `docs/PROJECT_GUIDE.md` (full spec, architecture, feature
catalog) and `docs/CLOUD-MODE.md` (the cloud API contract).** This file says where things stand, how to work
on the project, and exactly what to build next. It contains no secrets and must never contain any (the repo
is public, with history).

Last updated: 2026-09-27, after test build `v1.0.0-test.10` was dispatched.

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
| Phases 4–7, 9–12 (files & knowledge base, local documents, research+, writing/learning, Mac control, automation, voice/windows, privacy & polish) | ⏳ planned — details in `docs/PROJECT_GUIDE.md` |

Test builds are GitHub pre-releases `v1.0.0-test.N` (latest: test.10). The owner installs them on the M4 Air
and reports back with screenshots; this environment can't run macOS.

## 3. Next work, in order

### 3.1 Check test.10
`release.yml` for `v1.0.0-test.10` and `mac-engine.yml` on the branch head. test.10 is the first macOS build of
the `keyring` crate (Keychain). If it fails, fix it before anything else.

### 3.2 Owner's new requests (from the last session — do these next)

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
- **Ask the owner:** can anyone sign up on byteai.bytebylogan.xyz (is there a public/free tier)? If sign-up is
  invite-only, the onboarding must say so instead of implying anyone can use the cloud.

**B. Cloud as its own tab, not a toggle.**
- Replace the composer's Cloud/This Mac pill with a top-level workspace switch in the sidebar header:
  **This Mac · Cloud · Both** (persist as a setting, e.g. `workspace`).
- **Cloud tab:** the chat list comes from the server (`GET /api/conversations`, source of truth), with a
  **New conversation** button (`POST /api/conversations`) and **Delete** per conversation. Delete isn't in
  `docs/CLOUD-MODE.md` — try `DELETE /api/conversations/{id}` and confirm with the owner. Opening one loads
  `GET /api/conversations/{id}`. Mode buttons are the account's `me.modes` (Fast / Auto / Extended /
  Extended+ as the key allows). Reuse the existing streaming (`cloud::cmd::send`, `cloud::follow`), message
  actions and documents; keep a local mirror only for offline reading/search.
- **This Mac tab:** today's local experience, unchanged.

**C. "Both" tab: cloud and local together, for better and faster replies.**
- Recommended design: send each question to **both**; show the local answer as soon as it streams (fast), and
  the cloud answer side by side when it arrives (usually better). Reuse the side-by-side compare UI that
  already exists for multiple local models (`store.answer` with a shared `group`, `compare-grid`). The user
  picks which answer continues the conversation ("Keep this one"); default to the cloud answer when it
  finishes, the local one if the cloud is unreachable.
- Optional later: local writes a quick draft while the cloud "refines" it (send the draft as context).
- Private chats never go to the cloud, in any tab.

### 3.3 Then Phase 4 — Files & knowledge base (local)
The owner chose "reads my files" and a personal knowledge base early on. Plan (not started):
1. **File attachments in local chats** (`src-tauri/src/files/{mod,pdf,office,text}.rs`, new): drop/paste/pick
   in the composer → `file_ingest(path)` → `{name, kind, pages, text, truncated}`. PDF via `pdf-extract`,
   DOCX/PPTX/XLSX via `zip` + `quick-xml`, TXT/MD/CSV/JSON/code as UTF-8, HTML via `dom_smoothie`. Caps: 25 MB,
   200 pages. Short files go in whole; long ones are chunked and the best passages picked with
   `tools::fetch::relevant_passages` (then embeddings once item 3 exists). Store attachment text (capped) in
   the message JSON so reloads and edits work. (The cloud tab already has its own attachments — share the chip
   UI in `src/components/chat/Attachments.tsx`.)
2. **OCR** (`files/ocr.rs`): Apple Vision `VNRecognizeTextRequest` via `objc2-vision` behind
   `#[cfg(target_os = "macos")]`, Linux stub returns "OCR needs macOS". Scanned PDFs: render pages with PDFKit
   (`objc2-pdf-kit`) and OCR them.
3. **Embeddings engine**: an embedding-mode llama-server (`--embedding --pooling mean`, catalog helper
   `nomic-embed-v1.5`, ~0.15 GB) via the existing `Extras`/`Engine`, started on demand and stopped after 5 idle
   minutes; `embed(texts) -> Vec<Vec<f32>>` batching `/v1/embeddings`.
4. **Knowledge base** (`kb.rs`, **DB schema v4** — v3 is taken by `cloud_id`): `kb_sources` (folder/file, last
   scan), `kb_chunks` (text, file, page, heading, f32 embedding BLOB) + FTS5. Indexer walks folders (skip
   hidden/huge/binary), ~700-token chunks with overlap, progress events `kb://progress`; `notify` watcher
   re-indexes changes. Hybrid search: BM25 + cosine merged by reciprocal-rank fusion.
5. **Agent tool `search_my_files`**: offered when the KB has content and a "My files" composer toggle is on;
   results are numbered sources (file + page). Questions mentioning "my notes/files/docs" force a first KB
   search (same pattern as the forced web search in `agent.rs`).
6. **Reader view** (`components/reader/Reader.tsx`): side panel with the extracted text, cited passage
   highlighted and scrolled into view.
7. **Settings → Knowledge base** tab: folders, file counts, index status, re-index, storage used.

Verify with small fixture files in `src-tauri/tests/fixtures/` (PDF, DOCX, PPTX, XLSX, CSV), unit tests for the
chunker, RRF merge, migration v4 and cosine, a real-engine embeddings test (nomic-embed, cached in
`mac-engine.yml`), vitest for the UI, screenshots, then a test build for the owner to try OCR and a real folder.

### 3.4 After that
Phases 5–12 per PROJECT_GUIDE. The owner prioritizes **answer quality**, then stability, then looks, then
feature count.

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
- **CI:** `ci.yml` (frontend + Linux Rust tests + secret scan) on every push; `mac-engine.yml` (real engine on
  macOS/Metal) only when engine/chat/agent/tools/speed/tune/summarize/Cargo.lock/LLAMA_TAG change.
- **Test build:** `node scripts/bump.mjs 1.0.0-test.N` → commit → push → run the `release.yml` workflow with
  input `tag = v1.0.0-test.N` (workflow_dispatch). Add a CHANGELOG entry per build, in plain language.
- **Style:** match the surrounding code; no repo-wide reformatting (there is no prettier config — don't run
  prettier on whole files). Tests next to the code (Rust unit tests; `*.test.ts` for TS).

## 5. Map of the code (where things live)

- Engine & speed: `src-tauri/src/engine.rs` (launch args, `LaunchOpts`, `Draft`, retry without helper),
  `tune.rs` (per-Mac tuning), `speed.rs` (measurement), `models.rs` (catalog, helpers, scoring, offload plan),
  `system.rs` (RAM/GPU planner, `plan_offload`, GPU share), `chip.rs` (speed estimates).
- Catalog: `src-tauri/catalog/models.json`, built by `scripts/build-catalog.mjs` from
  `scripts/catalog-sources.json` + `catalog-discovered.json` (includes `speedHead`).
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
