#!/usr/bin/env bash
# Assemble Caco.app from an already-built binary. macOS only: it shells out to
# iconutil and codesign, none of which exist elsewhere, plus resvg
# (`brew install resvg`) to rasterise the icon. Nothing here
# compiles anything, so the caller decides how the binary was built.
#
#   VERSION=4.1.0 contrib/macos/bundle.sh [binary] [outdir]
#
# Defaults to target/release/caco and dist/. VERSION only reaches Info.plist,
# and defaults to the workspace version in Cargo.toml — the same one the binary
# itself reports.
set -euo pipefail

BIN="${1:-target/release/caco}"
OUTDIR="${2:-dist}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n1)}"

[[ -f "$BIN" ]] || { echo "no binary at $BIN" >&2; exit 1; }

APP="$OUTDIR/Caco.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# --- icon ------------------------------------------------------------------
# Every size is rendered straight from assets/caco.svg rather than scaled from
# one large bitmap. resvg is the same renderer build.rs uses for the window
# icon, so the Dock and the window cannot disagree.
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
ICONSET="$SCRATCH/caco.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    resvg -w "$size" -h "$size" assets/caco.svg \
        "$ICONSET/icon_${size}x${size}.png"
    resvg -w "$((size * 2))" -h "$((size * 2))" assets/caco.svg \
        "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/caco.icns"

# --- executable ------------------------------------------------------------
# The real binary is caco-bin and CFBundleExecutable is a wrapper, because a
# Finder-launched app inherits only /usr/bin:/bin:/usr/sbin:/sbin. Caco resolves
# sourceports off PATH and sourceports/doctor.rs runs `brew list`, so without this the
# app finds nothing from the Dock while working fine from a terminal.
install -m 755 "$BIN" "$APP/Contents/MacOS/caco-bin"

cat > "$APP/Contents/MacOS/caco" <<'WRAPPER'
#!/bin/sh
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
exec "$(dirname "$0")/caco-bin" "$@"
WRAPPER
chmod 755 "$APP/Contents/MacOS/caco"

# --- Info.plist ------------------------------------------------------------
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Caco</string>
    <key>CFBundleDisplayName</key><string>Caco</string>
    <key>CFBundleIdentifier</key><string>com.mythofmeat.caco</string>
    <key>CFBundleExecutable</key><string>caco</string>
    <key>CFBundleIconFile</key><string>caco</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# --- signature -------------------------------------------------------------
# Ad-hoc, not notarised. An arm64 binary with no signature at all is killed by
# the kernel on launch, so this is required rather than cosmetic.
codesign --force --sign - "$APP"

echo "$APP"
