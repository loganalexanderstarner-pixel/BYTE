#!/usr/bin/env bash
# Builds the pinned sherpa-onnx speaker-diarization tool (speaker labels in
# transcripts, speakers.rs) as one self-contained binary (static onnxruntime)
# and puts it where Tauri expects the sidecar:
# src-tauri/binaries/sherpa-diarize-<target-triple>.
#
# Usage: scripts/build-sherpa.sh            # host triple
#        TARGET_TRIPLE=aarch64-apple-darwin scripts/build-sherpa.sh
set -euo pipefail

SHERPA_TAG="${SHERPA_TAG:-$(cat "$(dirname "$0")/SHERPA_TAG")}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT/src-tauri/binaries"
TRIPLE="${TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}"
WORK="${SHERPA_WORK:-$ROOT/.cache/sherpa-onnx}"
SRC="$WORK/src-$SHERPA_TAG"
BUILD="$SRC/build"
BIN="$BUILD/bin/sherpa-onnx-offline-speaker-diarization"

mkdir -p "$WORK" "$OUT_DIR"

if [ ! -f "$BIN" ]; then
  if [ ! -d "$SRC" ]; then
    echo "==> Fetching sherpa-onnx $SHERPA_TAG"
    git clone --quiet --depth 1 --branch "$SHERPA_TAG" https://github.com/k2-fsa/sherpa-onnx "$SRC"
  fi

  FLAGS=(
    -DCMAKE_BUILD_TYPE=Release
    -DBUILD_SHARED_LIBS=OFF
    -DSHERPA_ONNX_ENABLE_BINARY=ON
    -DSHERPA_ONNX_ENABLE_PYTHON=OFF
    -DSHERPA_ONNX_ENABLE_TESTS=OFF
    -DSHERPA_ONNX_ENABLE_CHECK=OFF
    -DSHERPA_ONNX_ENABLE_PORTAUDIO=OFF
    -DSHERPA_ONNX_ENABLE_WEBSOCKET=OFF
    -DSHERPA_ONNX_ENABLE_TTS=OFF
    -DSHERPA_ONNX_ENABLE_C_API=OFF
  )
  case "$TRIPLE" in
    *-apple-darwin)
      FLAGS+=(-DCMAKE_OSX_ARCHITECTURES=arm64 -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3)
      JOBS="$(sysctl -n hw.logicalcpu)"
      ;;
    *)
      JOBS="$(nproc)"
      ;;
  esac

  echo "==> Configuring sherpa-onnx ($TRIPLE)"
  # SHERPA_CMAKE_EXTRA: extra cmake arguments (e.g. FETCHCONTENT_SOURCE_DIR_* where downloads are blocked).
  # shellcheck disable=SC2086
  cmake -S "$SRC" -B "$BUILD" "${FLAGS[@]}" ${SHERPA_CMAKE_EXTRA:-} >/dev/null
  echo "==> Building the speaker-diarization tool with $JOBS jobs"
  cmake --build "$BUILD" --config Release --target sherpa-onnx-offline-speaker-diarization -j "$JOBS"
fi

DEST="$OUT_DIR/sherpa-diarize-$TRIPLE"
cp "$BIN" "$DEST"
chmod +x "$DEST"
echo "==> Installed $DEST"
