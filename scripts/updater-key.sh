#!/usr/bin/env bash
# Release step: prepares the updater signing key (secret TAURI_SIGNING_PRIVATE_KEY, given as $KEY).
# Removes the spaces and line breaks a terminal copy can add, checks the key is a readable Tauri/minisign
# secret key that its password (secret TAURI_SIGNING_PRIVATE_KEY_PASSWORD, as $PASSWORD) unlocks, and only then hands it to the build (masked) and turns the signed update files on.
# Writes `args=` to $GITHUB_OUTPUT; never fails the release.
set -uo pipefail
out="${GITHUB_OUTPUT:-/dev/stdout}"
env_file="${GITHUB_ENV:-/dev/null}"

key=$(printf '%s' "${KEY:-}" | tr -d ' \t\r\n')
if [ -z "$key" ]; then
  echo "args=" >> "$out"
  echo "::notice::No TAURI_SIGNING_PRIVATE_KEY secret, so this release has no one-click update files."
  exit 0
fi
echo "::add-mask::$key"

decoded=$(printf '%s' "$key" | base64 -d 2>/dev/null) || decoded=""
if [[ "$decoded" != "untrusted comment:"* ]]; then
  echo "args=" >> "$out"
  echo "::warning::The TAURI_SIGNING_PRIVATE_KEY secret isn't a readable signing key (${#key} characters; a whole one is about 350). It was probably cut off when copied. Copy the whole ~/.tauri/byte-updater.key file again (pbcopy < ~/.tauri/byte-updater.key on a Mac) and update the secret. This release has no one-click update files."
  exit 0
fi

# The password must unlock it: sign a scrap file the way the build will.
scrap=$(mktemp)
echo byte > "$scrap"
if ! TAURI_SIGNING_PRIVATE_KEY="$key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${PASSWORD:-}" npx --no-install tauri signer sign "$scrap" >/dev/null 2>&1; then
  rm -f "$scrap" "$scrap.sig"
  echo "args=" >> "$out"
  echo "::warning::The TAURI_SIGNING_PRIVATE_KEY_PASSWORD secret doesn't unlock the signing key. Update it with the password you chose when you made the key. This release has no one-click update files."
  exit 0
fi
rm -f "$scrap" "$scrap.sig"

echo "TAURI_SIGNING_PRIVATE_KEY=$key" >> "$env_file"
echo "args=--config '{\"bundle\":{\"createUpdaterArtifacts\":true}}'" >> "$out"
echo "Signing key found (${#key} characters); this release includes signed update files."
