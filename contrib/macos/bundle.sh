#!/usr/bin/env bash
# Assemble Caco.app from an already-built binary. macOS only: it shells out to
# sips, iconutil and codesign, none of which exist elsewhere. Nothing here
# compiles anything, so the caller decides how the binary was built.
#
#   VERSION=4.1.0 contrib/macos/bundle.sh [binary] [outdir]
#
# Defaults to target/release/caco and dist/. VERSION only reaches Info.plist —
# the binary reports whatever was sed'd into Cargo.toml, exactly as on Arch.
set -euo pipefail

BIN="${1:-target/release/caco}"
OUTDIR="${2:-dist}"
VERSION="${VERSION:-0.0.0}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

[[ -f "$BIN" ]] || { echo "no binary at $BIN" >&2; exit 1; }

APP="$OUTDIR/Caco.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# --- icon ------------------------------------------------------------------
# assets/caco.png is 1024x1024, which is exactly the largest size an .icns
# wants, so every variant is a downscale and none are invented.
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
ICONSET="$SCRATCH/caco.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/caco.png \
        --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    sips -z "$((size * 2))" "$((size * 2))" assets/caco.png \
        --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/caco.icns"

# --- executable ------------------------------------------------------------
# The real binary is caco-bin and CFBundleExecutable is a wrapper, because a
# Finder-launched app inherits only /usr/bin:/bin:/usr/sbin:/sbin. Caco resolves
# sourceports off PATH and ports/doctor.rs runs `brew list`, so without this the
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
