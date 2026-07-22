#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

PREFIX="${PREFIX:-/usr/local}"
BINDIR="$PREFIX/bin"

BINARIES=(
    "caco"
    "caco-gui"
    "caco-tui"
)

echo "Building caco (release)..."
cargo build --release --workspace

for bin in "${BINARIES[@]}"; do
    echo "Installing to $BINDIR..."
    sudo install -Dm755 target/release/"$bin" "$BINDIR/$bin"
done

echo "Installing desktop entry and icon..."
sudo install -Dm644 assets/caco.desktop "$PREFIX/share/applications/caco.desktop"
sudo install -Dm644 assets/caco.png     "$PREFIX/share/icons/hicolor/256x256/apps/caco.png"

sudo gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" || true

echo "Installed:"
ls -lh "$BINDIR/caco" "$BINDIR/caco-gui" "$BINDIR/caco-tui"
echo
