#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
BINDIR="$PREFIX/bin"

echo "Building caco (release)..."
cargo build --release --workspace

echo "Installing to $BINDIR..."
sudo install -Dm755 target/release/caco     "$BINDIR/caco"
sudo install -Dm755 target/release/caco-gui "$BINDIR/caco-gui"
sudo install -Dm755 target/release/caco-tui "$BINDIR/caco-tui"

echo "Installing desktop entry and icon..."
sudo install -Dm644 assets/caco.desktop "$PREFIX/share/applications/caco.desktop"
# 256x256 is a standard hicolor size every GTK icon cache scans.
sudo install -Dm644 assets/caco.png     "$PREFIX/share/icons/hicolor/256x256/apps/caco.png"
# Scalable fallback so high-DPI launchers pick the large source directly.
sudo install -Dm644 assets/caco.png     "$PREFIX/share/icons/hicolor/scalable/apps/caco.png"

# Refresh the icon cache so launchers pick up the new icon without a logout.
# Non-fatal: not every system has gtk-update-icon-cache, and the icon
# is still usable by apps that read the filesystem directly.
sudo gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" || true

echo "Installed:"
ls -lh "$BINDIR/caco" "$BINDIR/caco-gui" "$BINDIR/caco-tui"
echo
caco --version
