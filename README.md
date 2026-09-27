<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" alt="BYTE logo" />
</p>

<h1 align="center">BYTE</h1>
<p align="center"><b>A private AI assistant that runs entirely on your Mac.</b><br/>
No account. No subscription. Your conversations never leave your computer.</p>

---

## What it does today

- **Chats with a capable AI model running on your Mac's GPU** (Apple Silicon + Metal), even offline.
- **Four modes:** ⚡ Fast · 🎚 Auto · 🔭 Deep · 🚀 Extended. They set how long BYTE thinks and how thorough the answer is.
- **Visible thinking:** turn reasoning on or off per message and open "Thought for…" to see how BYTE worked it out.
- **Up to date with sources:** BYTE searches and reads the web when a question needs current information, and shows numbered, clickable citations and source cards. No account or API key needed.
- **Exact math:** a built-in calculator handles arithmetic, percentages and unit conversions.
- **Guided setup:** checks your Mac, recommends the right model, and downloads it with pause and resume. Every file is checksum-verified.
- **Memory-aware:** BYTE works out how much memory each model needs and won't run one that doesn't fit. It shrinks the context window automatically when memory is tight.
- **11 themes**, adjustable text size and density, keyboard shortcuts, and saved chat history with search.

BYTE is being built in phases. PDF/PowerPoint/Word export, file and knowledge-base reading, Mac app control, voice, and more arrive in upcoming test builds, delivered by auto-update. See [the roadmap](#roadmap).

## Requirements

| | Minimum | Recommended |
|---|---|---|
| Mac | Apple Silicon (M1 or newer) | M2 / M3 / M4 |
| Memory | 8 GB (Fast model only) | **16 GB** or more |
| macOS | 13.3 Ventura | 14 Sonoma or newer |
| Free disk | 7 GB | 12 GB |

## Install

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

> Test builds (tags like `v1.0.0-test.1`) are marked **Pre-release** on the Releases page.

## Models

| Model | Download | Best for | Speed on a base M4 |
|---|---|---|---|
| **Qwen3 14B** (default) | 9.0 GB | Best reasoning that fits in 16 GB | ~12–15 tokens/s |
| Qwen3 8B | 5.0 GB | Faster replies, a little less capable | ~22–28 tokens/s |
| Qwen3 30B-A3B | 14.7 GB | Macs with 24 GB+ | ~30–40 tokens/s |

Models are stored in `~/Library/Application Support/com.loganstarner.byte/models/` and can be deleted from **Settings → Models**.

**Honest expectations:** a 14B model running locally is very capable, but it won't match the largest cloud models at writing or reasoning. It's fast for everyday questions. Deep and Extended answers can take a minute or more, especially on a fanless MacBook Air during long runs.

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
3. Encrypted chat database, memory, projects, profiles, export
4. File drop (PDF, Word, images with OCR) and personal knowledge base
5. PDF / PowerPoint / Word / web-page documents with themes and charts
6. Deep and Extended research, academic search, fact-check, compare, trip planner, YouTube
7. Writing studio, long-form writer, flashcards, quizzes, tutor, translate
8. Speed: speculative decoding, smart routing, model lab (add any GGUF)
9. Mac control: Notes, Reminders, Calendar, Mail, Messages, Music, Safari, Shortcuts, files
10. Automation: tasks, scheduler, daily briefing, page watchers, storage and battery tools
11. Voice, vision, Quick Ask, menu-bar mini chat, command palette, notes, mind maps
12. Offline switch, Touch ID lock, permission dashboard, kids mode, iCloud backup, **v1.0**

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
| Set the version everywhere | `node scripts/bump.mjs 1.0.0-test.2` |
| Publish a test build | bump, commit, then `git tag v1.0.0-test.2 && git push --tags` |

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
