#!/usr/bin/env bash
# Builds the pinned llama.cpp `llama-server` as a single self-contained binary
# (static libs, Metal shaders embedded) and installs it where Tauri expects the
# sidecar: src-tauri/binaries/llama-server-<target-triple>.
#
# Usage: scripts/build-llama-server.sh            # host triple
#        TARGET_TRIPLE=aarch64-apple-darwin scripts/build-llama-server.sh
set -euo pipefail

LLAMA_TAG="${LLAMA_TAG:-$(cat "$(dirname "$0")/LLAMA_TAG")}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT/src-tauri/binaries"
TRIPLE="${TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}"
WORK="${LLAMA_WORK:-$ROOT/.cache/llama.cpp}"
SRC="$WORK/src-$LLAMA_TAG"
BUILD="$SRC/build"

mkdir -p "$WORK" "$OUT_DIR"

if [ ! -f "$BUILD/bin/llama-server" ]; then
  if [ ! -d "$SRC" ]; then
    echo "==> Fetching llama.cpp $LLAMA_TAG"
    git clone --quiet --depth 1 --branch "$LLAMA_TAG" https://github.com/ggml-org/llama.cpp "$SRC"
  fi

  FLAGS=(
    -DCMAKE_BUILD_TYPE=Release
    -DBUILD_SHARED_LIBS=OFF
    -DLLAMA_OPENSSL=OFF
    -DLLAMA_BUILD_TESTS=OFF
    -DLLAMA_BUILD_EXAMPLES=OFF
  )
  case "$TRIPLE" in
    *-apple-darwin)
      # NATIVE=OFF keeps Apple clang's default apple-m1 baseline so the binary
      # runs on every M-series chip regardless of which runner built it.
      FLAGS+=(
        -DGGML_METAL=ON
        -DGGML_METAL_EMBED_LIBRARY=ON
        -DGGML_NATIVE=OFF
        -DCMAKE_OSX_ARCHITECTURES=arm64
        -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3
      )
      JOBS="$(sysctl -n hw.logicalcpu)"
      ;;
    *)
      FLAGS+=(-DGGML_NATIVE=ON)
      JOBS="$(nproc)"
      ;;
  esac

  echo "==> Configuring ($TRIPLE)"
  cmake -S "$SRC" -B "$BUILD" "${FLAGS[@]}" >/dev/null
  echo "==> Building llama-server with $JOBS jobs"
  cmake --build "$BUILD" --config Release --target llama-server -j "$JOBS"
fi

DEST="$OUT_DIR/llama-server-$TRIPLE"
cp "$BUILD/bin/llama-server" "$DEST"
chmod +x "$DEST"

# The binary must not depend on anything outside the system.
if [[ "$TRIPLE" == *-apple-darwin ]]; then
  if otool -L "$DEST" | tail -n +2 | grep -vE '^\s*(/usr/lib/|/System/Library/)'; then
    echo "error: llama-server links against non-system libraries" >&2
    exit 1
  fi
fi

"$DEST" --version 2>&1 | head -n 2 || true
echo "==> Installed $DEST"
