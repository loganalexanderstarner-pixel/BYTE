#!/usr/bin/env bash
# Builds BYTE on your own Mac, exactly like the release workflow does, with no
# GitHub Actions minutes: engine (llama-server with Metal) → app → .dmg.
#
#   git clone https://github.com/loganalexanderstarner-pixel/BYTE && cd BYTE
#   git checkout claude/new-session-tu1a5x
#   scripts/build-mac.sh            # build the .dmg
#   scripts/build-mac.sh --install  # build, then copy BYTE.app to /Applications
#
# Needs (one-time): Xcode Command Line Tools (`xcode-select --install`),
# Homebrew (https://brew.sh) for cmake and node, and Rust (https://rustup.rs).
# The first build takes 10–20 minutes (llama.cpp and Rust compile); later
# builds reuse the compiled engine and take a few minutes.
set -euo pipefail
cd "$(dirname "$0")/.."

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1; }

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "This script builds the Apple Silicon Mac app; run it on an M-series Mac." >&2
  exit 1
fi

say "1/5 Checking tools"
xcode-select -p >/dev/null 2>&1 || { echo "Install Xcode Command Line Tools first: xcode-select --install" >&2; exit 1; }
if ! need cmake || ! need node; then
  need brew || { echo "Install Homebrew first (https://brew.sh), then run this again." >&2; exit 1; }
  brew install cmake node
fi
if ! need cargo; then
  echo "Installing Rust (rustup)…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi
rustup target add aarch64-apple-darwin >/dev/null

say "2/5 Building the engines (llama-server and whisper-cli with Metal, the speaker-label tool; cached after the first time)"
TARGET_TRIPLE=aarch64-apple-darwin scripts/build-llama-server.sh
TARGET_TRIPLE=aarch64-apple-darwin scripts/build-whisper.sh
TARGET_TRIPLE=aarch64-apple-darwin scripts/build-sherpa.sh

say "3/5 Installing app dependencies"
npm ci

say "4/5 Building BYTE"
npx tauri build --target aarch64-apple-darwin

say "5/5 Checking the result"
app=$(ls -d src-tauri/target/aarch64-apple-darwin/release/bundle/macos/*.app | head -n1)
dmg=$(ls src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/*.dmg | head -n1)
test -x "$app/Contents/MacOS/llama-server"
test -x "$app/Contents/MacOS/whisper-cli"
test -x "$app/Contents/MacOS/sherpa-diarize"
codesign --verify --deep --strict "$app"
"$app/Contents/MacOS/llama-server" --version >/dev/null 2>&1 || true
echo "App: $app"
echo "DMG: $dmg"

if [[ "${1:-}" == "--install" ]]; then
  say "Installing to /Applications"
  osascript -e 'tell application "BYTE" to quit' >/dev/null 2>&1 || true
  rm -rf /Applications/BYTE.app
  cp -R "$app" /Applications/
  # Built on this Mac, so macOS doesn't need to ask "Open Anyway".
  xattr -cr /Applications/BYTE.app || true
  echo "Installed. Open BYTE from Applications."
fi
