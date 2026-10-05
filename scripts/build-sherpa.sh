#!/usr/bin/env bash
# Builds the pinned sherpa-onnx tools as self-contained binaries (static
# onnxruntime) and puts them where Tauri expects the sidecars:
#   src-tauri/binaries/sherpa-diarize-<target-triple>  speaker labels (speakers.rs)
#   src-tauri/binaries/sherpa-tts-<target-triple>      BYTE's voices, Kokoro (tts.rs)
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
GEN=()
# Windows: executables carry a suffix and the Visual Studio generator writes
# into a per-configuration subdirectory.
case "$TRIPLE" in
  *-windows-msvc) EXE=".exe"; BIN_SUBDIR="Release/" ;;
  *)              EXE="";     BIN_SUBDIR="" ;;
esac
BIN="$BUILD/bin/${BIN_SUBDIR}sherpa-onnx-offline-speaker-diarization$EXE"
TTS="$BUILD/bin/${BIN_SUBDIR}sherpa-onnx-offline-tts$EXE"

mkdir -p "$WORK" "$OUT_DIR"

if [ ! -f "$BIN" ] || [ ! -f "$TTS" ]; then
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
    -DSHERPA_ONNX_ENABLE_TTS=ON
    -DSHERPA_ONNX_ENABLE_C_API=OFF
  )
  case "$TRIPLE" in
    *-apple-darwin)
      FLAGS+=(-DCMAKE_OSX_ARCHITECTURES=arm64 -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3)
      JOBS="$(sysctl -n hw.logicalcpu)"
      ;;
    *-windows-msvc)
      # No -G: CMake defaults to the newest Visual Studio it finds (see
      # build-llama-server.sh for why 17 2022 must not be hard-coded).
      GEN=(-A x64)
      JOBS="${NUMBER_OF_PROCESSORS:-8}"
      ;;
    *)
      JOBS="$(nproc)"
      ;;
  esac

  echo "==> Configuring sherpa-onnx ($TRIPLE)"
  # SHERPA_CMAKE_EXTRA: extra cmake arguments (e.g. FETCHCONTENT_SOURCE_DIR_* where downloads are blocked).
  # shellcheck disable=SC2086
  cmake -S "$SRC" -B "$BUILD" ${GEN[@]+"${GEN[@]}"} "${FLAGS[@]}" ${SHERPA_CMAKE_EXTRA:-} >/dev/null
  echo "==> Building the speaker-diarization and speech tools with $JOBS jobs"
  cmake --build "$BUILD" --config Release --target sherpa-onnx-offline-speaker-diarization sherpa-onnx-offline-tts -j "$JOBS"
fi

for pair in "$BIN:sherpa-diarize" "$TTS:sherpa-tts"; do
  DEST="$OUT_DIR/${pair##*:}-$TRIPLE$EXE"
  cp "${pair%%:*}" "$DEST"
  chmod +x "$DEST"
  echo "==> Installed $DEST"
done
