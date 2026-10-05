#!/usr/bin/env bash
# Builds the pinned llama.cpp `llama-server` for Android (arm64-v8a) with the NDK
# and installs it where the APK packs native libraries:
#   src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a/libllama-server.so
#   src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a/libllama-server-i8mm.so
#
# Why an executable named like a library: Android (10+) only lets an app run
# programs from its native library folder (W^X), and only files named lib*.so are
# installed there. With android:extractNativeLibs="true" the file lands in
# nativeLibraryDir and `engine.rs` starts it like the desktop sidecar, so every
# chat, tuning and speed path stays the same.
#
# Two CPU builds, picked at run time from /proc/cpuinfo:
#   baseline  armv8.2-a + dotprod + fp16: every phone from about 2019 on
#   i8mm      armv8.6-a + i8mm: newer cores, much faster prompt reading
#
# Usage: ANDROID_NDK_HOME=<ndk> scripts/build-llama-android.sh
set -euo pipefail

LLAMA_TAG="${LLAMA_TAG:-$(cat "$(dirname "$0")/LLAMA_TAG")}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${LLAMA_WORK:-$ROOT/.cache/llama.cpp}"
SRC="$WORK/src-$LLAMA_TAG"
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
  echo "==> Fetching llama.cpp $LLAMA_TAG"
  git clone --quiet --depth 1 --branch "$LLAMA_TAG" https://github.com/ggml-org/llama.cpp "$SRC"
fi

build() { # <name> <march>
  local name="$1" march="$2" build="$SRC/build-android-$1"
  if [ ! -f "$build/bin/llama-server" ]; then
    echo "==> Configuring $name ($march)"
    cmake -S "$SRC" -B "$build" \
      -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
      -DANDROID_ABI="$ABI" -DANDROID_PLATFORM="android-$API" -DANDROID_STL=c++_static \
      -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
      -DLLAMA_OPENSSL=OFF -DLLAMA_CURL=OFF -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF \
      -DGGML_OPENMP=OFF -DGGML_NATIVE=OFF -DGGML_LLAMAFILE=ON \
      -DCMAKE_C_FLAGS="-march=$march" -DCMAKE_CXX_FLAGS="-march=$march" >/dev/null
    echo "==> Building $name"
    cmake --build "$build" --target llama-server -j "$(nproc)"
  fi
  local dest="$OUT_DIR/$3"
  cp "$build/bin/llama-server" "$dest"
  "$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip" "$dest"
  # Only Android system libraries may be needed at run time.
  if "$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf" -d "$dest" | grep NEEDED \
      | grep -vE '\[(libc|libm|libdl|liblog|libandroid)\.so\]'; then
    echo "error: $dest needs a library Android doesn't ship" >&2
    exit 1
  fi
  echo "==> Installed $dest ($(du -h "$dest" | cut -f1))"
}

build baseline "armv8.2-a+dotprod+fp16" libllama-server.so
build i8mm "armv8.6-a+dotprod+fp16+i8mm" libllama-server-i8mm.so
