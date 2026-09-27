# CLAUDE.md — working on BYTE

BYTE is a **local-first AI assistant for Apple Silicon Macs** (target: MacBook Air M4, 16 GB). It ships its own
inference engine (llama.cpp `llama-server`, Metal) as a Tauri sidecar, downloads Qwen3 GGUF models on first launch,
and runs everything on-device. Only web search/reading and model downloads use the internet.

Read `docs/PROJECT_GUIDE.md` for the full feature catalog, architecture, decisions, and phase status.

## Non-negotiable decisions (from the owner)

- **macOS only, Apple Silicon only.** No Windows/Intel work.
- **Never require a paid Apple Developer account.** Builds are ad-hoc / self-signed. README explains "Open Anyway".
- **The assistant's name is BYTE.** It never calls itself Qwen or another model (it may say it runs Qwen3 locally
  if asked). Tone: normal, friendly, direct — *not* cyberpunk-talk. The *visual* style is neon/cyberpunk.
- **Answer quality and research depth win** trade-offs, then stability, then looks, then feature count.
- Every feature is a **module** the user can toggle; disabled modules add no prompt text, tools, or memory use.
- Answers are well structured: headings, numbered steps, bullets, TL;DR box for long answers, callouts.
- Unique BYTE layout with a **command-deck home**; 20 themes; smooth 60 fps animations.
- Work happens on branch `claude/new-session-tu1a5x`; test builds are tags `v1.0.0-test.N` (pre-releases).

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
  src/system.rs           hardware info + RAM planner (fit, context size)
  src/chat.rs             SSE streaming client, context fitting, cancellation, e2e test
  src/router.rs           per-turn thinking/length plan by mode
  src/prompt.rs           system prompt (identity, date, mode rules)
  src/commands.rs         #[tauri::command] wrappers (thin)
  src/settings.rs         persisted settings (camelCase JSON)
  binaries/               sidecars, built by scripts (gitignored)
scripts/                  build-llama-server.sh, LLAMA_TAG (pinned), bump.mjs
.github/workflows/        ci.yml (frontend + macOS Rust + real-engine e2e), release.yml (dmg)
```

## Commands

| Task | Command |
|---|---|
| Install JS deps | `npm install` |
| Build engine sidecar for this machine | `scripts/build-llama-server.sh` |
| Run the app | `npm run tauri dev` |
| Frontend checks | `npm run typecheck && npx vitest run && npm run build` |
| Rust tests | `cd src-tauri && cargo test` |
| Real-engine e2e | `BYTE_TEST_LLAMA_SERVER=<bin> BYTE_TEST_MODEL=<Qwen3-0.6B gguf> cargo test e2e -- --ignored` |
| Set version everywhere | `node scripts/bump.mjs 1.0.0-test.N` |
| Publish a test build | bump → commit → `git tag v1.0.0-test.N && git push origin v1.0.0-test.N` |

On Linux, Tauri needs `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev`.
`tauri-build` requires a sidecar file to exist at `src-tauri/binaries/llama-server-<host-triple>`.

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
