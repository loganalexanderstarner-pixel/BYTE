# BYTE as a Homebrew cask. This repo is its own tap:
#   brew tap loganalexanderstarner-pixel/byte https://github.com/loganalexanderstarner-pixel/BYTE
#   brew install --cask byte
# scripts/bump.mjs keeps the version in step with the app.
cask "byte" do
  version "0.12.6"
  sha256 :no_check

  url "https://github.com/loganalexanderstarner-pixel/BYTE/releases/download/v#{version}/BYTE_#{version}_aarch64.dmg"
  name "BYTE"
  desc "Local-first AI assistant that runs on your Mac"
  homepage "https://github.com/loganalexanderstarner-pixel/BYTE"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: ">= :ventura"

  app "BYTE.app"

  # BYTE isn't signed with a paid Apple Developer ID, so macOS would block it
  # the first time; installing with Homebrew skips the "Open Anyway" step.
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{appdir}/BYTE.app"]
  end

  zap trash: [
    "~/Library/Application Support/com.loganstarner.byte",
    "~/Library/Caches/com.loganstarner.byte",
    "~/Library/Preferences/com.loganstarner.byte.plist",
    "~/Library/Saved Application State/com.loganstarner.byte.savedState",
  ]
end
