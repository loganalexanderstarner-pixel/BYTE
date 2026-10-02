# CLAUDE.md — working on BYTE

BYTE is a **local-first AI assistant for Apple Silicon Macs** (target: MacBook Air M4, 16 GB). It ships its own
inference engine (llama.cpp `llama-server`, Metal) as a Tauri sidecar, downloads Qwen3 GGUF models on first launch,
and runs everything on-device. Only web search/reading and model downloads use the internet.

**Start with `docs/HANDOFF.md`** (current state, next tasks, how to work here), then `docs/PROJECT_GUIDE.md` for the
full feature catalog, architecture, decisions, and phase status, `docs/DESIGN-AND-PLATFORMS.md` (byte-ai's design
tokens + logo, and the Windows/Linux plan), `docs/CLOUD-MODE.md` (BYTE as a remote backend) and
`docs/CLUSTER-REQUESTS.md` (what the app still needs from the cluster; public repo, so no internals in it).

## Non-negotiable decisions (from the owner)

- **macOS and Apple Silicon first — but Windows and Linux are now targets too**
  (changed by the owner, 2026-09-27; superseded "macOS only, no Windows/Intel work").
  Each should feel native to its own OS rather than one build for all three.
  **No iOS/iPadOS app** — dropped by the owner 2026-09-27; `PROJECT_GUIDE.md` was
  right to exclude it. Three desktop platforms, nothing else.
- **Never require a paid Apple Developer account.** Builds are ad-hoc / self-signed. README explains "Open Anyway".
- **The assistant's name is BYTE.** It never calls itself Qwen or another model (it may say it runs Qwen3 locally
  if asked). Tone: normal, friendly, direct — *not* cyberpunk-talk. The *visual* style is neon/cyberpunk.
- **Answer quality and research depth win** trade-offs, then stability, then looks, then feature count.
- Every feature is a **module** the user can toggle; disabled modules add no prompt text, tools, or memory use.
- Answers are well structured: headings, numbered steps, bullets, TL;DR box for long answers, callouts.
- Unique BYTE layout with a **command-deck home**; 20 themes; smooth 60 fps animations.
- Work happens on branch `claude/new-session-tu1a5x`; builds are tagged by phase: **v0.<phase>.<n>** (`v0.5.0` = Phase 5, `v0.5.1` = a Phase 5 improvement build; `v1.0.0` = finished). Each release's title and description come from `docs/releases/v<version>.md`; `docs/VERSIONS.md` lists every version (owner decision 2026-09-28; before that, builds were `v1.0.0-test.1…14`).

## Layout

```
src/                      React 19 + TypeScript UI (Vite)
  app/                    App root + Shell layout
  components/             chat/, onboarding/, settings/, models/, Sidebar, EngineBadge
  design/                 Logo.tsx, themes.ts
  lib/                    api.ts (typed IPC), types.ts (mirrors Rust), format.ts, markdown.ts
  state/store.ts          zustand store: settings, engine, downloads, conversations, streaming
  styles/                 tokens.css (theme variables), app.css (components)
src-tauri/                Rust core (Tauri 2)
  src/lib.rs              builder, plugins, startup (engine preload, stale-engine reaping, signal handling)
  src/engine.rs           llama-server supervisor (port, API key, health, warm-up, restart, PID file)
  src/models.rs           catalog + resumable SHA-256-verified downloader
  src/db.rs               encrypted SQLite (SQLCipher): chats, FTS search, memories
  src/export.rs           export chats to Markdown + JSON
  src/summarize.rs        automatic chat title / summary / tags (local model, JSON)
  src/profiles.rs         profiles (separate data folders; models shared)
  src/chip.rs             Apple Silicon chip detection + speed estimates
  src/speed.rs            real tokens/sec measurement (writing + prompt reading)
  src/tune.rs             per-Mac, per-model engine tuning (boost, KV precision, batch size)
  src/modelcfg.rs         per-family sampling + thinking control (model-card recommendations)
  src/system.rs           hardware info + RAM planner (fit, context size)
  src/chat.rs             SSE streaming client, context fitting, cancellation, e2e test
  src/files.rs            attached files → text (PDF, Office, text/code) and photos (data URLs) for the model
  src/ocr.rs              text in photos / scanned PDFs (Apple Vision + PDFKit; macOS only)
  src/embed.rs            embedding engine (nomic-embed, own llama-server, on demand, idle stop)
  src/kb.rs               knowledge base: folders → passages → FTS5 + vectors, hybrid search, rescans
  src/voice.rs            voice input: whisper-cli sidecar, speech models in <models>/voice, transcribe recordings/files
  src/speakers.rs         speaker labels in transcripts: sherpa-diarize sidecar + 2 small models, matched to whisper segments
  src/media.rs            video helper: yt-dlp (downloaded on request, checksum-checked) for videos without captions
  src/tts.rs              BYTE's voices: speaking pipeline (chunks, pauses, fades) + one continuous audio stream (cpal)
  src/voices.rs           voice catalog (catalog/voices.json: Kokoro, Piper, Kitten, Supertonic, Pocket), per-engine args
  src/notes.rs            notes: Markdown files in Documents/BYTE/Notes, web clipper (byte://clip), KB source
  src/mindmap.rs          mind maps: Markdown → tree (no model) or a model outline
  src/board.rs            brainstorm boards (DB): stickies, model ideas / themes
  src/help.rs             help articles (src/help/*.md) for "how do I … in BYTE" questions
  src/offline.rs          the offline switch: a connector layer on every internet client (holds through proxies)
  src/lock.rs             Touch ID lock (LocalAuthentication), idle timer, `ensure` gate on data commands
  src/privacy.rs          Settings → Privacy: Mac permission status, activity log (actions.jsonl)
  src/kids.rs             kids mode: PIN (PBKDF2), kid-safe prompt, enforced in backend + chat commands (Kids folder)
  src/backup.rs           encrypted backups (ring PBKDF2 + ChaCha20-Poly1305) to iCloud Drive, restore at next launch, auto-delete, erase
  src/speech.rs           answers read aloud (macOS `say`, text via a temp file), speakable(), voices
  src/wake.rs             "Hey BYTE": native mic (cpal, macOS), voice-burst detector, whisper check, opens Quick Ask
  src/quick.rs            Quick Ask window, menu-bar icon, global shortcuts from settings
  src/macctl.rs           Mac control: fixed AppleScripts (user words only as argv), approval card, undo
  src/router.rs           per-turn thinking/length plan by mode
  src/prompt.rs           system prompt (identity, date, mode rules)
  src/backend.rs          ModelBackend trait: LocalLlama + Cloud, fallback rules (chat_send goes through it)
  src/cloud/              cloud mode (docs/CLOUD-MODE.md): client + SSE follower, Keychain key store, commands, voice.rs (cloud voices)
  src/commands.rs         #[tauri::command] wrappers (thin)
  src/settings.rs         persisted settings (camelCase JSON)
  binaries/               sidecars, built by scripts (gitignored)
scripts/                  build-llama-server.sh, LLAMA_TAG (pinned), bump.mjs
.github/workflows/        ci.yml (frontend + Linux Rust tests + secret scan), mac-engine.yml (real engine on macOS, on engine changes),
                          release.yml (dmg, workflow_dispatch with a tag)
```

## Commands

| Task | Command |
|---|---|
| Install JS deps | `npm install` |
| Build engine sidecars for this machine | `scripts/build-llama-server.sh`, `scripts/build-whisper.sh` (voice) and `scripts/build-sherpa.sh` (speaker labels) |
| Run the app | `npm run tauri dev` |
| Frontend checks | `npm run typecheck && npx vitest run && npm run build` |
| Rust tests | `cd src-tauri && cargo test` |
| Real-engine e2e | `BYTE_TEST_LLAMA_SERVER=<bin> BYTE_TEST_MODEL=<Qwen3-0.6B gguf> cargo test e2e -- --ignored` |
| Set version everywhere | `node scripts/bump.mjs 0.<phase>.<n>` |
| Publish a build | write `docs/releases/v0.P.N.md` (first line `# Phase P — … (v0.P.N)`, then new / what it can do / not yet) + a row in `docs/VERSIONS.md` → bump → commit → push → run `release.yml` (workflow_dispatch, `tag=v0.P.N`) |
| Build on the owner's Mac (no Actions minutes) | `scripts/build-mac.sh` (`--install` copies BYTE.app to /Applications) |
| Secret scan | `scripts/check-secrets.sh` (also in CI) |

On Linux, Tauri needs `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev`.
`tauri-build` requires sidecar files to exist at `src-tauri/binaries/llama-server-<host-triple>` and
`src-tauri/binaries/whisper-cli-<host-triple>`, `src-tauri/binaries/sherpa-diarize-<host-triple>` and
`src-tauri/binaries/sherpa-tts-<host-triple>` (empty placeholders are fine for `cargo test`).

## Conventions

- Rust owns every side effect (processes, files, network). The UI only calls commands and listens to events.
- IPC types: Rust enums use `rename_all = "camelCase", rename_all_fields = "camelCase"`; mirror them in
  `src/lib/types.ts`. `chat::wire_format` tests lock the JSON shapes.
- Streaming uses `tauri::ipc::Channel`; long-lived status uses global events (`engine://status`, `models://download`).
- The local engine HTTP client must **not** reuse connections (`chat::local_client`) — llama-server closes them
  after streaming and reuse caused every other request to fail.
- Model output is untrusted: render through `lib/markdown.ts` (DOMPurify), open links externally.
- Colors come only from theme tokens in `styles/tokens.css`. Respect `prefers-reduced-motion`.
- Add tests with every module: Rust unit tests next to the code, vitest `*.test.ts` next to TS files.
- Keep `CHANGELOG.md` and `docs/PROJECT_GUIDE.md` (phase status) updated each phase.
- **Log every commit in `docs/WORKLOG.md`** (what, why, files, how to verify, how to undo), so a broken change
  can be reverted on its own instead of going back to an old copy of the repo.
- **The repo is public (2026-09-28), so GitHub Actions is free**: CI runs on every push (docs-only pushes skip
  it), the Mac engine test runs when engine code changes, test builds run `release.yml` by hand. Still run
  `scripts/check-all.sh` before pushing so CI stays green. `scripts/build-mac.sh` builds on the owner's Mac too.
- **No secrets in the repo, ever** (it's public, with history). BYTE cloud keys live only in the macOS Keychain
  (`cloud::keychain`); tests use short fake keys (`byte_test_…`). Run `scripts/check-secrets.sh` before pushing
  (CI runs it too). Never use a real key someone pastes into a chat; tell them to revoke it.
- Cloud mode follows `docs/CLOUD-MODE.md`: modes come from `me.modes`, stream with `delta` events, the document
  outline approval step is required, and an unreachable cloud falls back to the local model.
