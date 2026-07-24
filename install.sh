#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

# Embed the current commit into `--version`. Passed via env (not read from .git by
# the build script) so a no-op `git pull` can't trigger a needless release rebuild.
CACO_GIT_HASH="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
if ! git diff --quiet HEAD 2>/dev/null; then
    CACO_GIT_HASH="$CACO_GIT_HASH-dirty"
fi
export CACO_GIT_HASH

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
