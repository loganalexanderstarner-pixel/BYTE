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

mkdir -p "$WORK" "$OUT_DIR"

if [ ! -f "$BUILD/bin/whisper-cli" ]; then
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
    *)
      JOBS="$(nproc)"
      ;;
  esac

  echo "==> Configuring whisper.cpp ($TRIPLE)"
  cmake -S "$SRC" -B "$BUILD" "${FLAGS[@]}" >/dev/null
  echo "==> Building whisper-cli with $JOBS jobs"
  cmake --build "$BUILD" --config Release --target whisper-cli -j "$JOBS"
fi

DEST="$OUT_DIR/whisper-cli-$TRIPLE"
cp "$BUILD/bin/whisper-cli" "$DEST"
chmod +x "$DEST"
echo "==> Installed $DEST"
