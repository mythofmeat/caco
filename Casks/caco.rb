# This repository is its own tap:
#   brew tap mythofmeat/caco https://github.com/mythofmeat/caco
#   brew install --cask mythofmeat/caco/caco
# The release workflow's `cask` job rewrites version and sha256 after it has
# uploaded the zip, so this file always names the newest published release.
cask "caco" do
  version "4.1.7"
  sha256 "c0195db3f46dc9727d96a0e3ca85bd31a3ba8a4e3d687ae70cf9f229f057bb99"

  url "https://github.com/mythofmeat/caco/releases/download/v#{version}/caco-#{version}-macos-arm64.zip"
  name "Caco"
  desc "Doom WAD library manager"
  homepage "https://github.com/mythofmeat/caco"

  depends_on arch: :arm64
  depends_on :macos

  app "Caco.app"
  # The real binary, not the bundle's PATH-extending wrapper: a terminal already
  # has Homebrew on PATH, and the wrapper finds caco-bin beside $0, which is this
  # symlink's directory rather than the bundle's.
  binary "#{appdir}/Caco.app/Contents/MacOS/caco-bin", target: "caco"

  # The bundle is signed ad hoc, not notarised, so Gatekeeper refuses to open a
  # quarantined copy. Steps take {{appdir}}, not Ruby interpolation, and run
  # sandboxed: the app is the only thing xattr may write.
  postflight_steps do
    run "/usr/bin/xattr",
        args:           ["-dr", "com.apple.quarantine", "{{appdir}}/Caco.app"],
        writable_paths: ["Caco.app"],
        writable_base:  :appdir
  end

  zap trash: [
    "~/Library/Application Support/caco",
    "~/Library/Caches/caco",
  ]
end
