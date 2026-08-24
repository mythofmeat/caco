#!/usr/bin/env bash
set -eu
cd "$(dirname "${BASH_SOURCE[0]}")"
srcdir="$(git rev-parse --show-toplevel)"

build() {
    cd "$srcdir/"
    cargo build --workspace --release
}

install_package() {
    cd "$srcdir/"
    INSTALL_PATH=$HOME/.local/bin

    install -Dm755 "./target/release/caco" "$INSTALL_PATH/caco"
}

build
install_package
