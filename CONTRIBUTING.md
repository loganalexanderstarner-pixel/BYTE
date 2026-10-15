# Contributing to BYTE

BYTE is a local-first AI assistant for Mac, Windows and Android (Linux planned): a Tauri 2 app (Rust core, React + TypeScript UI) that
ships its own inference engine (llama.cpp `llama-server`) as a sidecar and runs models on the device. Start with the
[README](README.md), then `docs/PROJECT_GUIDE.md` (features and architecture), `docs/DESIGN-AND-PLATFORMS.md` (design tokens and how
each platform differs) and `docs/CLOUD-MODE.md` (using BYTE Cloud as a backend).

## Principles

- **Same look everywhere, native behaviour:** identical colours, icons, layout and themes on every platform; dialogs, notifications,
  install and OS features follow each OS.
- **No paid developer account required to build or run it.** Builds are ad-hoc or self-signed; the README explains the first-launch step.
- **The assistant's name is BYTE.** It never calls itself another model. Tone: normal, friendly, direct.
- **Answer quality and research depth win** trade-offs, then stability, then looks, then feature count.
- **Every feature is a module** the user can toggle; disabled modules add no prompt text, tools or memory use. Anything that listens,
  looks at the screen or controls the computer starts off and has its own switch.
- Answers are well structured: headings, numbered steps, bullets, a summary box for long answers.
- Versions: `v0.<phase>.<n>`; each release's notes live in `docs/releases/v<version>.md` and `docs/VERSIONS.md` lists them all.

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
  src/diagnostics.rs      "Copy diagnostics": device, memory, engine, redacted log tail (no chats, no keys)
  src/privacy.rs          Settings → Privacy: Mac permission status, activity log (actions.jsonl)
  src/kids.rs             kids mode: PIN (PBKDF2), kid-safe prompt, enforced in backend + chat commands (Kids folder)
  src/backup.rs           encrypted backups (ring PBKDF2 + ChaCha20-Poly1305) to iCloud Drive, restore at next launch, auto-delete, erase
  src/speech.rs           answers read aloud (macOS `say`, text via a temp file), speakable(), voices
  src/wake.rs             "Hey BYTE": native mic (cpal, macOS), voice-burst detector, whisper check, opens Quick Ask
  src/quick.rs            Quick Ask window, menu-bar icon, global shortcuts from settings
  src/macctl.rs           Mac control: fixed AppleScripts (user words only as argv), approval card, undo; texts sent after an editable card
  src/messages.rs         Messages inbox (opt-in, Full Disk Access): chat.db read-only, Contacts names, new-text watcher, send into a thread
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
- **Describe each change in the commit message** (what and why), and note user-visible changes in `CHANGELOG.md`.
- **The repo is public (2026-09-28), so GitHub Actions is free**: CI runs on every push (docs-only pushes skip
  it), the Mac engine test runs when engine code changes, test builds run `release.yml` by hand. Still run
  `scripts/check-all.sh` before pushing so CI stays green. `scripts/build-mac.sh` builds on the owner's Mac too.
- **No secrets in the repo, ever** (it's public, with history). BYTE cloud keys live only in the macOS Keychain
  (`cloud::keychain`); tests use short fake keys (`byte_test_…`). Run `scripts/check-secrets.sh` before pushing
  (CI runs it too). Never use a real key someone pastes into a chat; tell them to revoke it.
- Cloud mode follows `docs/CLOUD-MODE.md`: modes come from `me.modes`, stream with `delta` events, the document
  outline approval step is required, and an unreachable cloud falls back to the local model.
