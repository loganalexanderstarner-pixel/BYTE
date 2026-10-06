#!/usr/bin/env bash
# Builds the pinned whisper.cpp `whisper-cli` (voice input, voice.rs) for Android (arm64-v8a) with the NDK and
# installs it where the APK packs native libraries, like the engine (build-llama-android.sh explains why a program
# is named lib*.so):
#   src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a/libwhisper-cli.so
#   src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a/libwhisper-cli-i8mm.so
# `bundled::tool` picks the i8mm build on phones whose cores all have it, else the baseline.
#
# Usage: ANDROID_NDK_HOME=<ndk> scripts/build-whisper-android.sh
set -euo pipefail

WHISPER_TAG="${WHISPER_TAG:-$(cat "$(dirname "$0")/WHISPER_TAG")}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${WHISPER_WORK:-$ROOT/.cache/whisper.cpp}"
SRC="$WORK/src-$WHISPER_TAG"
NDK="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
API="${ANDROID_API:-28}"
ABI=arm64-v8a
OUT_DIR="${OUT_DIR:-$ROOT/src-tauri/gen/android/app/src/main/jniLibs/$ABI}"

if [ -z "$NDK" ] || [ ! -f "$NDK/build/cmake/android.toolchain.cmake" ]; then
  echo "error: set ANDROID_NDK_HOME to an NDK (r27+)" >&2
  exit 1
fi

mkdir -p "$WORK" "$OUT_DIR"
if [ ! -d "$SRC" ]; then
  echo "==> Fetching whisper.cpp $WHISPER_TAG"
  git clone --quiet --depth 1 --branch "$WHISPER_TAG" https://github.com/ggml-org/whisper.cpp "$SRC"
fi

build() { # <name> <march> <output file name>
  local name="$1" march="$2" build="$SRC/build-android-$1"
  if [ ! -f "$build/bin/whisper-cli" ]; then
    echo "==> Configuring whisper $name ($march)"
    cmake -S "$SRC" -B "$build" \
      -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
      -DANDROID_ABI="$ABI" -DANDROID_PLATFORM="android-$API" -DANDROID_STL=c++_static \
      -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
      -DWHISPER_BUILD_TESTS=OFF -DWHISPER_SDL2=OFF \
      -DGGML_OPENMP=OFF -DGGML_NATIVE=OFF \
      -DCMAKE_C_FLAGS="-march=$march" -DCMAKE_CXX_FLAGS="-march=$march" >/dev/null
    echo "==> Building whisper-cli $name"
    cmake --build "$build" --target whisper-cli -j "$(nproc)"
  fi
  local dest="$OUT_DIR/$3"
  cp "$build/bin/whisper-cli" "$dest"
  "$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip" "$dest"
  # Only Android system libraries may be needed at run time.
  if "$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf" -d "$dest" | grep NEEDED \
      | grep -vE '\[(libc|libm|libdl|liblog|libandroid)\.so\]'; then
    echo "error: $dest needs a library Android doesn't ship" >&2
    exit 1
  fi
  echo "==> Installed $dest ($(du -h "$dest" | cut -f1))"
}

build baseline "armv8.2-a+dotprod+fp16" libwhisper-cli.so
build i8mm "armv8.6-a+dotprod+fp16+i8mm" libwhisper-cli-i8mm.so
