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
# ENGINE_BACKEND picks the accelerator: cuda (NVIDIA, fastest) or vulkan (every
# vendor, including the AMD and Intel GPUs we cannot test on). Each gets its own
# build directory and its own output name, so both can exist and be compared on
# the same machine -- which is the only way to answer whether the 500 MB of
# CUDA libraries in the installer is worth what it buys.
ENGINE_BACKEND="${ENGINE_BACKEND:-cuda}"
BUILD="$SRC/build-$ENGINE_BACKEND"
# Set by the per-platform case below; empty elsewhere.
GEN=()
case "$TRIPLE" in
  *-windows-msvc) EXE=".exe"; BIN_SUBDIR="Release/" ;;
  *)              EXE="";     BIN_SUBDIR="" ;;
esac

mkdir -p "$WORK" "$OUT_DIR"

if [ ! -f "$BUILD/bin/${BIN_SUBDIR}llama-server$EXE" ]; then
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
    *-windows-msvc)
      # GGML_NATIVE=OFF matters as much here as on Apple, and for the same
      # reason: this box is Zen 4, so NATIVE=ON bakes in AVX-512 and the
      # binary dies with an illegal instruction on any CPU without it --
      # which is most of them. A shipped engine must run on the machines
      # people actually have, not the one that compiled it.
      FLAGS+=(-DGGML_NATIVE=OFF)
      if [ "$ENGINE_BACKEND" = "vulkan" ]; then
        # Vulkan needs no vendor SDK at runtime: the user's own graphics driver
        # supplies the implementation. That is what lets one build cover
        # NVIDIA, AMD and Intel, including the two we have no hardware to test.
        FLAGS+=(-DGGML_VULKAN=ON)
      else
        # CUDA_ARCHITECTURES is deliberately narrow while iterating. 120 is
        # Blackwell (RTX 50-series), which is what W1 tests on. Each
        # architecture compiles separately, so the wider shipping set costs
        # real build time -- pay it when packaging, not on every build.
        #   ship: CUDA_ARCHS="75;80;86;89;120"
        FLAGS+=(-DGGML_CUDA=ON -DCMAKE_CUDA_ARCHITECTURES="${CUDA_ARCHS:-120}")
      fi
      # No -G on purpose: on Windows CMake defaults to the newest Visual
      # Studio it finds, which works on a VS 2022 machine and on the VS 2026
      # runner image alike. It was hard-coded to "Visual Studio 17 2022" and
      # failed outright when windows-latest moved to an image with only VS 2026.
      # A VS generator is still wanted over Ninja, which would need cl.exe on
      # PATH, which means a developer prompt an SSH session lacks.
      GEN=(-A x64)
      JOBS="${NUMBER_OF_PROCESSORS:-8}"
      ;;
    *)
      FLAGS+=(-DGGML_NATIVE=ON)
      JOBS="$(nproc)"
      ;;
  esac

  echo "==> Configuring ($TRIPLE)"
  cmake -S "$SRC" -B "$BUILD" ${GEN[@]+"${GEN[@]}"} "${FLAGS[@]}" >/dev/null
  echo "==> Building llama-server with $JOBS jobs"
  cmake --build "$BUILD" --config Release --target llama-server -j "$JOBS"
fi

# The default backend keeps the plain name Tauri's externalBin expects; any
# other backend is suffixed so it sits alongside rather than replacing it.
if [ "$ENGINE_BACKEND" = "cuda" ]; then
  DEST="$OUT_DIR/llama-server-$TRIPLE$EXE"
else
  DEST="$OUT_DIR/llama-server-$ENGINE_BACKEND-$TRIPLE$EXE"
fi
cp "$BUILD/bin/${BIN_SUBDIR}llama-server$EXE" "$DEST"
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
