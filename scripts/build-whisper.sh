#!/usr/bin/env bash
# Builds the pinned whisper.cpp `whisper-cli` (voice input, voice.rs) as one
# self-contained binary (static libs, Metal shaders embedded on a Mac) and puts
# it where Tauri expects the sidecar: src-tauri/binaries/whisper-cli-<target-triple>.
#
# Usage: scripts/build-whisper.sh            # host triple
#        TARGET_TRIPLE=aarch64-apple-darwin scripts/build-whisper.sh
set -euo pipefail

WHISPER_TAG="${WHISPER_TAG:-$(cat "$(dirname "$0")/WHISPER_TAG")}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT/src-tauri/binaries"
TRIPLE="${TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}"
WORK="${WHISPER_WORK:-$ROOT/.cache/whisper.cpp}"
SRC="$WORK/src-$WHISPER_TAG"
BUILD="$SRC/build"
GEN=()
# Windows: the executable has a suffix and the Visual Studio generator writes
# into a per-configuration subdirectory.
case "$TRIPLE" in
  aarch64-*-windows-msvc) EXE=".exe"; BIN_SUBDIR="" ;;
  *-windows-msvc) EXE=".exe"; BIN_SUBDIR="Release/" ;;
  *)              EXE="";     BIN_SUBDIR="" ;;
esac

mkdir -p "$WORK" "$OUT_DIR"

if [ ! -f "$BUILD/bin/${BIN_SUBDIR}whisper-cli$EXE" ]; then
  if [ ! -d "$SRC" ]; then
    echo "==> Fetching whisper.cpp $WHISPER_TAG"
    git clone --quiet --depth 1 --branch "$WHISPER_TAG" https://github.com/ggml-org/whisper.cpp "$SRC"
  fi

  FLAGS=(
    -DCMAKE_BUILD_TYPE=Release
    -DBUILD_SHARED_LIBS=OFF
    -DWHISPER_BUILD_TESTS=OFF
    -DWHISPER_SDL2=OFF
    # A portable baseline: a binary built for one machine's exact CPU can
    # crash with "illegal instruction" on another.
    -DGGML_NATIVE=OFF
  )
  case "$TRIPLE" in
    *-apple-darwin)
      FLAGS+=(
        -DGGML_METAL=ON
        -DGGML_METAL_EMBED_LIBRARY=ON
        -DCMAKE_OSX_ARCHITECTURES=arm64
        -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3
      )
      JOBS="$(sysctl -n hw.logicalcpu)"
      ;;
    *-windows-msvc)
      # No -G on purpose: on Windows CMake defaults to the newest Visual
      # Studio it finds, which works on a VS 2022 machine and on the VS 2026
      # runner image alike. It was hard-coded to "Visual Studio 17 2022" and
      # failed outright when windows-latest moved to an image with only VS 2026.
      # A VS generator is still wanted over Ninja, which would need cl.exe on
      # PATH, which means a developer prompt an SSH session lacks.
      # CPU only: speech-to-text runs on short clips, so a GPU build would add
      # a CUDA dependency to a binary that does not need one.
      # ARM64 is clang + Ninja, as in build-llama-server.sh (ggml rejects MSVC on ARM).
      case "$TRIPLE" in
        aarch64-*) GEN=(-G Ninja); FLAGS+=(-DCMAKE_TOOLCHAIN_FILE="$(cygpath -m "$ROOT/scripts/arm64-windows-llvm.cmake" 2>/dev/null || echo "$ROOT/scripts/arm64-windows-llvm.cmake")" -DGGML_OPENMP=OFF) ;;
        *)         GEN=(-A x64) ;;
      esac
      JOBS="${NUMBER_OF_PROCESSORS:-8}"
      ;;
    *)
      JOBS="$(nproc)"
      ;;
  esac

  echo "==> Configuring whisper.cpp ($TRIPLE)"
  cmake -S "$SRC" -B "$BUILD" ${GEN[@]+"${GEN[@]}"} "${FLAGS[@]}" >/dev/null
  echo "==> Building whisper-cli with $JOBS jobs"
  cmake --build "$BUILD" --config Release --target whisper-cli -j "$JOBS"
fi

DEST="$OUT_DIR/whisper-cli-$TRIPLE$EXE"
cp "$BUILD/bin/${BIN_SUBDIR}whisper-cli$EXE" "$DEST"
chmod +x "$DEST"
echo "==> Installed $DEST"
