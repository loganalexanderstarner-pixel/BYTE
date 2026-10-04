#!/usr/bin/env bash
# Everything CI checks, run locally (CI is manual-only to save Actions minutes).
# Run before every push: scripts/check-all.sh
set -euo pipefail
cd "$(dirname "$0")/.."
echo "== secret scan";   scripts/check-secrets.sh
echo "== typecheck";     npm run -s typecheck
echo "== frontend tests"; npx vitest run --reporter=dot
echo "== frontend build"; npm run -s build >/dev/null
echo "== rust tests";    (cd src-tauri && cargo test --lib -q 2>&1 | grep -E "^test result|FAILED|panicked" )
echo "== clippy";        (cd src-tauri && cargo clippy --all-targets -q 2>&1 | grep -E "^error" && exit 1 || true)
echo "All checks passed."
