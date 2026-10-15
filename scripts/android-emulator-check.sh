#!/usr/bin/env bash
# Starts the debug APK on an emulator at phone and Fold sizes and checks that the
# app stays up and lays out below the status bar, also after "folding" (a size
# change while it runs: the bug that made test-4's top bar sit under the status
# bar). Saves screenshots and the view tree to $OUT. Run by android.yml; it needs
# a running emulator or device (adb). It does not load a model: the engine runs
# on translated ARM there, which is far too slow to say anything about speed.
#
# Usage: scripts/android-emulator-check.sh <apk> [out-dir]
set -uo pipefail

APK="${1:?usage: android-emulator-check.sh <apk> [out-dir]}"
OUT="${2:-android-ui-out}"
PKG=com.loganstarner.byteapp
mkdir -p "$OUT"
problems=()

adb install -r "$APK" >/dev/null || { echo "::error title=Emulator::the APK didn't install"; exit 1; }
adb shell pm grant "$PKG" android.permission.POST_NOTIFICATIONS 2>/dev/null || true

launch() { adb shell am force-stop "$PKG"; adb shell monkey -p "$PKG" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1; }

# webview_top <name>: screenshot + view tree, prints "top bottom screen_height" of the WebView.
look() {
  local name="$1"
  adb exec-out screencap -p >"$OUT/$name.png"
  adb shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1
  adb pull /sdcard/ui.xml "$OUT/$name.xml" >/dev/null 2>&1
  python3 - "$OUT/$name.xml" "$(adb shell wm size | tail -1 | grep -oE '[0-9]+x[0-9]+$')" <<'PY'
import re, sys
xml = open(sys.argv[1], encoding="utf-8", errors="ignore").read()
_, h = (int(v) for v in sys.argv[2].split("x"))
m = re.search(r'class="android\.webkit\.WebView"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', xml)
print(f"{m.group(2)} {m.group(4)} {h}" if m else f"none 0 {h}")
PY
}

check() { # <label> <min top px>
  local label="$1" min="$2" top bottom height
  read -r top bottom height < <(look "$label")
  if [ "$top" = none ]; then
    problems+=("$label: no WebView on screen (the app may have crashed or not drawn)")
  else
    [ "$top" -ge "$min" ] || problems+=("$label: the page starts at y=$top, under the status bar (needs at least $min)")
    [ "$bottom" -le "$height" ] || problems+=("$label: the page ends at y=$bottom, below the screen ($height)")
  fi
  if adb logcat -d -b crash 2>/dev/null | grep -q "FATAL EXCEPTION\|Fatal signal"; then
    problems+=("$label: the app crashed (see $OUT/logcat.txt)")
  fi
}

run() { # <label> <WxH> <density>
  adb shell wm size "$2"; adb shell wm density "$3"
  launch; sleep 30
  check "$1" 40
}

run phone-1080x2400 1080x2400 420
# Fold: unfold while the app runs, then fold again, no restart.
adb shell wm size 2184x1968; adb shell wm density 420; sleep 6
check fold-unfolded 40
adb shell wm size 1080x2520; adb shell wm density 450; sleep 6
check fold-folded 40

adb logcat -d >"$OUT/logcat.txt" 2>&1
adb shell wm size reset; adb shell wm density reset

if [ "${#problems[@]}" -gt 0 ]; then
  printf '%s\n' "${problems[@]}"
  if [ -n "${GITHUB_ACTIONS:-}" ]; then
    msg=$(printf '%s%%0A' "${problems[@]}")
    echo "::error title=Android emulator check::$msg"
  fi
  exit 1
fi
echo "emulator check passed: phone, unfolded and folded all lay out below the status bar"
