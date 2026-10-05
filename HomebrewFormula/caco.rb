# frozen_string_literal: true

# This repository is its own tap:
#   brew tap mythofmeat/caco git@github.com:mythofmeat/caco.git
#   brew install mythofmeat/caco/caco
# The repository is private and the formula clones it over HTTPS, so git needs
# GitHub HTTPS credentials (`gh auth setup-git` sets them up).
# The release workflow moves the tag below with every release.
class Caco < Formula
  desc "Doom WAD library manager"
  homepage "https://github.com/mythofmeat/caco"
  url "https://github.com/mythofmeat/caco.git",
      tag: "v4.1.6"
  license "MIT"

  depends_on "resvg" => :build
  depends_on "rust" => :build
  depends_on :macos

  def install
    stage = buildpath/"homebrew-stage"
    system "cargo", "install", *std_cargo_args(root: stage, path: "crates/caco")

    ENV["VERSION"] = version.to_s
    system "contrib/macos/bundle.sh", stage/"bin/caco"
    prefix.install "dist/Caco.app"
    bin.write_exec_script prefix/"Caco.app/Contents/MacOS/caco"
  end

  def caveats
    <<~EOS
      Caco.app was installed to:
        #{prefix}/Caco.app

      Run it with `caco`, or open that app once and keep it in your Dock.
    EOS
  end

  test do
    assert_predicate prefix/"Caco.app/Contents/MacOS/caco", :executable?
  end
end
