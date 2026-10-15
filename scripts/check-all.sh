#!/usr/bin/env bash
# Everything CI checks, run locally. Run before every push.
#
# CI is NOT manual-only: Actions minutes are unlimited for public repositories,
# and ci.yml runs on every push. This script is still the right first move
# because it answers in seconds rather than two minutes, and because catching a
# problem before pushing beats catching it after -- a shell script that would
# not parse went to the branch on 2026-10-04 because the syntax check was run
# in the wrong order.
set -euo pipefail
cd "$(dirname "$0")/.."
echo "== secret scan";   scripts/check-secrets.sh
echo "== typecheck";     npm run -s typecheck
echo "== frontend tests"; npx vitest run --reporter=dot
echo "== frontend build"; npm run -s build >/dev/null
echo "== rust tests";    (cd src-tauri && cargo test --lib -q 2>&1 | grep -E "^test result|FAILED|panicked" )
echo "== clippy";        (cd src-tauri && cargo clippy --all-targets -q 2>&1 | grep -E "^error" && exit 1 || true)
echo "All checks passed."
