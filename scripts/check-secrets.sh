#!/usr/bin/env bash
# Fails if something that looks like a real credential is in the tracked files.
# This repository is public (with its history), so BYTE cloud keys and other
# secrets must never be committed; they live in the macOS Keychain.
#
# Allowed: short fake keys in tests ("byte_test_…") and documentation.
set -euo pipefail
cd "$(dirname "$0")/.."

patterns=(
  'byte_[A-Za-z0-9_-]{30,}'                 # BYTE cloud API keys
  'Bearer byte_[A-Za-z0-9_-]{20,}'
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
  'ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}'
  'sk-[A-Za-z0-9_-]{32,}'
  'AKIA[0-9A-Z]{16}'
)
found=0
for p in "${patterns[@]}"; do
  if hits=$(git grep --untracked -nIE -e "$p" -- . ':!scripts/check-secrets.sh' ':!*.lock' ':!package-lock.json' | grep -vE 'byte_test_[a-z_]+' ); then
    if [ -n "$hits" ]; then
      echo "Possible secret ($p):"
      echo "$hits" | sed 's/\(.\{160\}\).*/\1…/'
      found=1
    fi
  fi
done
if [ "$found" -ne 0 ]; then
  echo "Remove the secret (and rotate it if it was ever pushed)." >&2
  exit 1
fi
echo "No secrets found."
