#!/usr/bin/env bash
# Builds BYTE for Android in one go (docs/ANDROID.md): the engines with the NDK,
# then the APK. Every path is explicit, so it behaves the same in a fresh shell,
# over SSH or in CI (environment problems look like tool errors otherwise).
#
# Needs: ANDROID_HOME (SDK with platform 36 + build-tools), NDK_HOME (r27+),
# JAVA_HOME (JDK 17+), Rust with the aarch64-linux-android target, Node 22.
#
# Usage: scripts/build-android.sh            # debug APK (installs over adb as is)
#        scripts/build-android.sh --release  # release APK (signed when the keystore secrets exist)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
: "${NDK_HOME:?set NDK_HOME to the NDK (r27+)}"
: "${JAVA_HOME:?set JAVA_HOME to a JDK 17+}"
export ANDROID_HOME NDK_HOME JAVA_HOME ANDROID_NDK_HOME="$NDK_HOME"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"
LOG_DIR="${LOG_DIR:-$ROOT/.cache/android-logs}"
mkdir -p "$LOG_DIR"

MODE=(--debug)
[ "${1:-}" = "--release" ] && MODE=()

echo "==> Engines (log: $LOG_DIR/engines.log)"
"$ROOT/scripts/build-llama-android.sh" >"$LOG_DIR/engines.log" 2>&1 || { tail -20 "$LOG_DIR/engines.log"; exit 1; }

echo "==> APK (log: $LOG_DIR/apk.log)"
cd "$ROOT"
npx tauri android build --apk ${MODE[@]+"${MODE[@]}"} --target aarch64 >"$LOG_DIR/apk.log" 2>&1 || { grep -A15 "What went wrong\|^error" "$LOG_DIR/apk.log" | head -40; exit 1; }

find "$ROOT/src-tauri/gen/android/app/build/outputs/apk" -name "*.apk" -exec ls -lh {} \;
echo "==> Install on a connected phone: adb install -r <apk>"
