# This repository is its own tap:
#   brew tap mythofmeat/caco https://github.com/mythofmeat/caco
#   brew install --cask mythofmeat/caco/caco
# The release workflow's `cask` job rewrites version and sha256 after it has
# uploaded the zip, so this file always names the newest published release.
cask "caco" do
  version "4.1.6"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/mythofmeat/caco/releases/download/v#{version}/caco-#{version}-macos-arm64.zip"
  name "Caco"
  desc "Doom WAD library manager"
  homepage "https://github.com/mythofmeat/caco"

  depends_on arch: :arm64
  depends_on macos: ">= :big_sur"

  app "Caco.app"
  # The real binary, not the bundle's PATH-extending wrapper: a terminal already
  # has Homebrew on PATH, and the wrapper finds caco-bin beside $0, which is this
  # symlink's directory rather than the bundle's.
  binary "#{appdir}/Caco.app/Contents/MacOS/caco-bin", target: "caco"

  # The bundle is signed ad hoc, not notarised, so Gatekeeper refuses to open a
  # quarantined copy.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/Caco.app"]
  end

  zap trash: [
    "~/Library/Application Support/caco",
    "~/Library/Caches/caco",
  ]
end
