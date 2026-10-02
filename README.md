<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" alt="BYTE logo" />
</p>

<h1 align="center">BYTE</h1>
<p align="center"><b>A private AI assistant that runs entirely on your Mac.</b><br/>
No account. No subscription. Your conversations never leave your computer.</p>

---

## What it does

- **Chat with a capable AI model running on your Mac's GPU** (Apple Silicon + Metal), even offline. Four modes: ⚡ Fast · 🎚 Auto · 🔭 Deep · 🚀 Extended.
- **Research with sources:** web search and reading with numbered citations, deep research with papers, fact-checks, comparisons, trip plans, reviews, prices and YouTube summaries. No account or API key needed.
- **Your files and documents:** reads PDFs, Office files, scans and photos; searches your folders (knowledge base); writes real PDF, PowerPoint and Word documents.
- **Writing and learning:** a writing studio, long-form writer, translation, flashcards, quizzes and a tutor.
- **Your Mac:** reminders, calendar, notes, Mail and Messages drafts, music, settings, files and storage cleanup, always with your OK and an Undo.
- **Everyday helpers:** to-dos, schedules, a daily briefing, news feeds, page watchers, automations, trackers, Notion/Obsidian/calendar connectors.
- **Voice:** dictation, "Hey BYTE", spoken answers in 2,000+ free natural voices, hands-free talking.
- **Notes, mind maps and a brainstorm board**, Quick Ask (⌥Space), a menu-bar icon and a command palette (⌘K).
- **Private by design:** an offline switch, a Touch ID lock, an activity log of everything BYTE did, kids mode, encrypted backups to iCloud Drive, and erasing everything in one step.
- **Yours:** 20 themes or your own, personality settings, Reduce motion, and a help center that works offline.
- **Optional BYTE Cloud** (invite only) for bigger models, with your Mac as the fallback.

Every feature is a module you can turn off. See [docs/VERSIONS.md](docs/VERSIONS.md) for what each version added.

## Requirements

| | Minimum | Recommended |
|---|---|---|
| Mac | Apple Silicon (M1 or newer) | M2 / M3 / M4 |
| Memory | 8 GB (Fast model only) | **16 GB** or more |
| macOS | 13.3 Ventura | 14 Sonoma or newer |
| Free disk | 7 GB | 12 GB |

## Install

**With Homebrew** (skips the "Open Anyway" step below):

```sh
brew tap loganalexanderstarner-pixel/byte https://github.com/loganalexanderstarner-pixel/BYTE
brew install --cask byte
```

**Or by hand:**

1. Download the latest `BYTE_…_aarch64.dmg` from [Releases](../../releases).
2. Open the `.dmg` and drag **BYTE** into **Applications**.
3. **First launch.** BYTE isn't signed with a paid Apple Developer ID yet, so macOS blocks it the first time:
   - **macOS 15 Sequoia and later:** open BYTE, click **Done** on the warning, then open **System Settings → Privacy & Security**, scroll down and click **Open Anyway**. Confirm with your password or Touch ID.
   - **macOS 13–14:** right-click (or Control-click) BYTE in Applications, choose **Open**, then **Open** again.
   - If macOS says BYTE **"is damaged and can't be opened"**, run this once in Terminal, then open it normally:
     ```sh
     xattr -cr /Applications/BYTE.app
     ```
4. Follow the welcome guide. It downloads your model (about 9 GB for the recommended one) and you're ready.

**Updates:** BYTE checks for new versions once a day and installs them in one click (Settings → About), after
checking that the update was signed by BYTE's maker. Homebrew users can also run `brew upgrade --cask byte`.

> Versions follow the build plan: **v0.5.0 is Phase 5**, v0.5.1 a Phase 5 improvement, and so on up to v1.0.0. Each release says what it has, and [docs/VERSIONS.md](docs/VERSIONS.md) lists them all, so you can pick an older, simpler version if you want fewer features.

## Models

BYTE has a built-in **model catalog** (Settings → Models) that checks every model against your Mac's memory and
recommends the best one. Examples of what it picks:

| Your Mac | BYTE's pick | Download |
|---|---|---|
| 8 GB | Qwen3.5 4B (Q6_K) | 3.5 GB |
| 16 GB | Qwen3.5 9B (Q6_K) | 7.5 GB |
| 32 GB | Qwen3.8 27B (Q5_K_M) | 19.8 GB |
| 96–128 GB | Qwen3.8 27B (Q6_K) or gpt-oss 120B | 22–63 GB |

The catalog lists **332 chat models with 1,800 versions** (0.3 GB to 400+ GB, 77 of them mixture-of-experts),
each labelled *Great fit*, *Fits*, or *Needs N GB*, with the **estimated tokens/sec and typical answer time on your
Mac** (BYTE detects the chip — M1 to M5, Pro/Max/Ultra — so an M4 shows faster numbers than an M2). Search, sort
(best, newest, smallest, fastest) and filter by memory size, strength, MoE or downloaded. The list itself is small
(about 0.4 MB) and refreshes from the web, so new models appear without an app update. Model files download from
Hugging Face only when you choose them, can be deleted any time, and are stored in `~/Library/Application Support/com.loganstarner.byte/models/`.

To regenerate the catalog: `node scripts/discover-models.mjs` (finds models from trusted publishers), then
`node scripts/build-catalog.mjs` (reads exact file sizes, checksums and architecture from Hugging Face without
downloading the models). Hand-picked models and quality scores live in `scripts/catalog-sources.json`.

**Several models at once:** when memory allows, load a second (or third, or fourth) model alongside the main one.
Then choose which one answers in the chat box, or pick **Compare all** to see their answers side by side. BYTE
checks the combined memory before loading.

**About the Neural Engine:** chat models run on the GPU (llama.cpp with Metal), which is the fastest path on Apple
Silicon; the Neural Engine can't run these models efficiently. BYTE will use it through Apple's frameworks for
text recognition in images (OCR) and voice.

**Honest expectations:** local models are very capable but won't match the largest cloud models at writing or
reasoning. Deep and Extended answers can take a minute or more, especially on a fanless MacBook Air.

## Privacy

- The AI model runs on your Mac. Prompts, answers, and files are processed locally.
- BYTE only uses the internet to download models (from Hugging Face), and, once web search ships, to look things up when you ask a question that needs current information.
- The built-in engine only listens on `127.0.0.1` and requires a random per-launch key, so other apps can't use it.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| <kbd>Enter</kbd> / <kbd>Shift</kbd>+<kbd>Enter</kbd> | Send / new line |
| <kbd>⌘N</kbd> | New chat |
| <kbd>⌘,</kbd> | Settings |
| <kbd>⌘\\</kbd> | Show / hide sidebar |
| <kbd>Esc</kbd> | Stop generating |

## Troubleshooting

- **"Engine problem"**: open **Settings → Engine** to see the log and restart the engine. Out-of-memory errors mean another app is using a lot of RAM. Quit it or switch to Qwen3 8B.
- **Download stopped**: press **Resume**. BYTE continues where it left off, even after quitting.
- **Slow answers**: use **Fast** mode or turn **Thinking** off. Closing heavy apps (browsers with many tabs, video editors) frees memory bandwidth for the model.

## Roadmap

1. ✅ Foundation: built-in engine, model manager, chat, thinking, onboarding, themes
2. ✅ Web search and reading with citations, tool use, calculator
3. ✅ Encrypted chat database, memory, projects, profiles, export
4. ✅ File drop (PDF, Word, images with OCR) and personal knowledge base
5. ✅ PDF / PowerPoint / Word / web-page documents with themes and charts
6. ✅ Deep and Extended research, academic search, fact-check, compare, trip planner, YouTube
7. ✅ Writing studio, long-form writer, flashcards, quizzes, tutor, translate
8. ✅ Speed: speculative decoding, smart routing, model lab (add any GGUF)
9. ✅ Mac control: Notes, Reminders, Calendar, Mail, Messages, Music, Safari, Shortcuts, files
10. ✅ Automation: tasks, scheduler, daily briefing, page watchers, storage and battery tools
11. ✅ Voice, vision, Quick Ask, menu-bar mini chat, command palette, notes, mind maps
12. 🔄 Offline switch, Touch ID lock, permission dashboard, kids mode, iCloud backup, your own themes and auto-update are done; **v1.0** is next

## Development

```sh
npm install
scripts/build-llama-server.sh      # builds the engine sidecar for this machine (pinned in scripts/LLAMA_TAG)
npm run tauri dev                  # run the app
```

| Task | Command |
|---|---|
| Frontend type-check / tests / build | `npm run typecheck` · `npx vitest run` · `npm run build` |
| Rust tests | `cd src-tauri && cargo test` |
| End-to-end test with a real engine | `BYTE_TEST_LLAMA_SERVER=… BYTE_TEST_MODEL=… cargo test e2e -- --ignored` |
| Set the version everywhere | `node scripts/bump.mjs 0.5.1` |
| Publish a build | add `docs/releases/v0.5.1.md`, bump, commit, then run the Release workflow with `tag=v0.5.1` |

**Stack:** Tauri 2 (Rust) · React 19 + TypeScript + Vite · llama.cpp (`llama-server`, Metal) · Qwen3 GGUF models.

```
src/                 React UI (components, state, design tokens and themes)
src-tauri/src/       Rust core
  engine.rs          supervises the llama-server sidecar (start, health, warm-up, restart)
  models.rs          model catalog + resumable, checksum-verified downloader
  system.rs          hardware detection and the RAM planner
  chat.rs            streaming chat (SSE), context-window fitting, cancellation
  router.rs          per-message thinking/length decisions, time-sensitive detection
  agent.rs           tool loop: forced search + auto-read, tool rounds, sources, stats
  tools/             web search (DuckDuckGo/Bing), page reader (SSRF-safe), calculator, action log
scripts/             engine build, version bump
```
