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
# Signing keystores (Android) and key files must never be committed, whatever they contain.
if files=$(git ls-files --cached --others --exclude-standard -- '*.jks' '*.keystore' '*.p12' '*.pfx' '*.key'); then
  if [ -n "$files" ]; then
    echo "Signing key files must not be in the repo:"
    echo "$files"
    found=1
  fi
fi
# The other half of the policy in docs/CLUSTER-REQUESTS.md: no internal
# hostnames, addresses or cluster layout. Credentials had a scanner; this did
# not, and three documents had drifted into naming internal services, a
# Kubernetes ConfigMap and the home network's machines before anyone noticed.
#
# Scoped to prose (*.md) on purpose. Source code legitimately contains private
# address ranges -- the SSRF guard's own tests assert that 192.168.x and 10.x
# are REFUSED, and flagging those would punish the code for being careful.
infra=(
  '(^|[^0-9.])(192\.168|10\.[0-9]{1,3}\.|172\.(1[6-9]|2[0-9]|3[01])\.)[0-9]{1,3}\.[0-9]{1,3}'
  '\.svc(\.cluster\.local)?\b'
  '\b[a-z][a-z0-9]*-node\b'
  'kubectl |ClusterIP|NodePort|kube-system|sealed-?secret'
)
for p in "${infra[@]}"; do
  if hits=$(git grep --untracked -nIE -e "$p" -- '*.md' ':!scripts/check-secrets.sh' | grep -vE 'setup-node'); then
    if [ -n "$hits" ]; then
      echo "Internal infrastructure detail in a public document ($p):"
      echo "$hits" | sed 's/\(.\{160\}\).*/\1…/'
      echo "  Describe the public API only. See docs/CLUSTER-REQUESTS.md."
      found=1
    fi
  fi
done

if [ "$found" -ne 0 ]; then
  echo "Remove the secret (and rotate it if it was ever pushed)." >&2
  exit 1
fi
echo "No secrets found."
