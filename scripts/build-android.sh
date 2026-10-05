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

# Things this script needs but used to leave to the caller, so it worked in CI
# and failed on a plain machine -- the opposite of what the header above promises:
#
# 1. Vendored OpenSSL (SQLCipher) is built by its own Makefile, which looks for
#    "aarch64-linux-android-ranlib" on PATH. NDK r23+ no longer ships binutils
#    under that name, only llvm-ar and llvm-ranlib, so it failed with "ranlib: not
#    found". The target-specific variables below are what cc-rs and openssl-src
#    read; ci.yml sets the same ones for its Android check.
NDK_BIN="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
export AR_aarch64_linux_android="${AR_aarch64_linux_android:-$NDK_BIN/llvm-ar}"
export RANLIB_aarch64_linux_android="${RANLIB_aarch64_linux_android:-$NDK_BIN/llvm-ranlib}"

# 2. The platform the Gradle project compiles against. Fail here, with the fix,
#    instead of twenty minutes into a build.
COMPILE_SDK=$(grep -oE 'compileSdk = [0-9]+' "$ROOT/src-tauri/gen/android/app/build.gradle.kts" | grep -oE '[0-9]+' || echo 36)
[ -d "$ANDROID_HOME/platforms/android-$COMPILE_SDK" ] || {
  echo "error: Android platform $COMPILE_SDK is not installed. Run:" >&2
  echo "  \"$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager\" \"platforms;android-$COMPILE_SDK\" \"build-tools;$COMPILE_SDK.0.0\"" >&2
  exit 1
}

# 3. tauri-build refuses to run unless every sidecar named in tauri.conf.json
#    exists, but on Android the engine ships as a library in jniLibs and the
#    sidecars are never used. Empty placeholders (gitignored) satisfy it; a real
#    file is left alone. android.yml does the same.
mkdir -p "$ROOT/src-tauri/binaries"
for b in llama-server whisper-cli sherpa-diarize sherpa-tts; do
  [ -e "$ROOT/src-tauri/binaries/$b-aarch64-linux-android" ] || : > "$ROOT/src-tauri/binaries/$b-aarch64-linux-android"
done
LOG_DIR="${LOG_DIR:-$ROOT/.cache/android-logs}"
mkdir -p "$LOG_DIR"

MODE=(--debug)
[ "${1:-}" = "--release" ] && MODE=()

echo "==> Engines (log: $LOG_DIR/engines.log)"
"$ROOT/scripts/build-llama-android.sh" >"$LOG_DIR/engines.log" 2>&1 || { tail -20 "$LOG_DIR/engines.log"; exit 1; }

echo "==> APK (log: $LOG_DIR/apk.log)"
cd "$ROOT"
npx tauri android build --apk ${MODE[@]+"${MODE[@]}"} --target aarch64 >"$LOG_DIR/apk.log" 2>&1 || {
  # Show the real cause when it can be found, and the end of the log when it
  # cannot. The grep alone printed nothing at all when it matched nothing, so a
  # failed build looked like a silent success -- in CI and by hand.
  grep -A15 "What went wrong\|^error\|doesn't exist\|not found" "$LOG_DIR/apk.log" | head -40
  echo "---- last lines of $LOG_DIR/apk.log ----"; tail -25 "$LOG_DIR/apk.log"
  exit 1
}

find "$ROOT/src-tauri/gen/android/app/build/outputs/apk" -name "*.apk" -exec ls -lh {} \;
echo "==> Install on a connected phone: adb install -r <apk>"
