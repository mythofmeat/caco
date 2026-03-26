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

echo "Installed:"
ls -lh "$BINDIR/caco" "$BINDIR/caco-gui" "$BINDIR/caco-tui"
echo
caco --version
