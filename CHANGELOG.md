# Changelog

## 1.0.0-test.1

First test build of the rebuilt BYTE, for Apple Silicon Macs.

- Built-in AI engine (llama.cpp with Metal); Ollama is no longer needed.
- Model catalog with Qwen3 14B (default), 8B and 30B-A3B, plus helper models.
- Resumable, checksum-verified model downloads with progress, speed and time left.
- RAM planner: checks each model against your Mac's memory, blocks models that can't run, and fits the context window automatically.
- Streaming chat with Fast / Auto / Deep / Extended modes and a thinking toggle; thinking shown in a collapsible panel.
- Tokens-per-second and timing under each answer; stop, copy, and regenerate.
- Welcome guide: Mac check, model choice, download, tips.
- New neon BYTE logo and app icon; 11 themes; text size and density settings.
- Chat history saved on this Mac, grouped by date and searchable.
- Engine settings: status, log, restart, context window size.
- macOS-only release pipeline producing a self-contained `.dmg`.
