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
