#!/usr/bin/env bash
# Build caco binaries with PyInstaller.
#
# Usage:
#   ./scripts/build.sh          # build CLI+TUI only
#   ./scripts/build.sh gui      # build full GUI bundle
#   ./scripts/build.sh all      # build both
#   ./scripts/build.sh clean    # remove build artifacts
#
# Output goes to dist/caco/ (CLI+TUI) and dist/caco-gui/ (GUI).
# Tarballs are created in dist/ as caco-{target}-{os}-{arch}.tar.gz.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
VERSION="$(uv run python -c 'from caco import __version__; print(__version__)')"

build_cli() {
    echo "==> Building CLI+TUI (caco.spec)..."
    uv run pyinstaller caco.spec --noconfirm --clean
    echo "==> Packaging dist/caco-${VERSION}-${OS}-${ARCH}.tar.gz"
    tar czf "dist/caco-${VERSION}-${OS}-${ARCH}.tar.gz" -C dist caco/
    echo "==> CLI+TUI build complete ($(du -sh dist/caco/ | cut -f1))"
}

build_gui() {
    echo "==> Installing GUI dependencies..."
    uv sync --extra gui
    echo "==> Building GUI (caco-gui.spec)..."
    uv run pyinstaller caco-gui.spec --noconfirm --clean
    echo "==> Packaging dist/caco-gui-${VERSION}-${OS}-${ARCH}.tar.gz"
    tar czf "dist/caco-gui-${VERSION}-${OS}-${ARCH}.tar.gz" -C dist caco-gui/
    echo "==> GUI build complete ($(du -sh dist/caco-gui/ | cut -f1))"
}

clean() {
    echo "==> Cleaning build artifacts..."
    rm -rf build/ dist/
    echo "==> Clean complete"
}

case "${1:-cli}" in
    cli)
        build_cli
        ;;
    gui)
        build_gui
        ;;
    all)
        build_cli
        build_gui
        ;;
    clean)
        clean
        ;;
    *)
        echo "Usage: $0 {cli|gui|all|clean}"
        exit 1
        ;;
esac
