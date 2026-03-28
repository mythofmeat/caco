#!/bin/bash
# Build caco-git package and upload to Gitea's Arch package registry.
#
# Usage: ./contrib/build-and-repo.sh
#
# Requires GITEA_TOKEN env var (personal access token with package write scope).
#
# One-time pacman.conf setup:
#
#   [eshen]
#   SigLevel = Optional TrustAll
#   Server = http://localhost:3000/api/packages/eshen/arch/$repo/$arch
#
# Then: sudo pacman -Sy caco-git

set -euo pipefail

GITEA_URL="${GITEA_URL:-http://localhost:3000}"
GITEA_OWNER="${GITEA_OWNER:-eshen}"
GITEA_REPO_NAME="eshen"  # pacman repo name (the [section] in pacman.conf)
SCRIPT_DIR="$(cd "$(dirname "$0")/arch" && pwd)"

if [[ -z "${GITEA_TOKEN:-}" ]]; then
    echo "Error: GITEA_TOKEN env var is required (Gitea personal access token)"
    exit 1
fi

# Build the package
cd "$SCRIPT_DIR"
makepkg -sf --noconfirm

# Upload to Gitea
for pkg in *.pkg.tar.zst; do
    echo "Uploading $pkg to Gitea..."
    curl --fail --user "${GITEA_OWNER}:${GITEA_TOKEN}" \
         --upload-file "$pkg" \
         "${GITEA_URL}/api/packages/${GITEA_OWNER}/arch/${GITEA_REPO_NAME}"
    echo ""
    echo "Uploaded $pkg"
    rm -f "$pkg"
done

echo ""
echo "Done! Package available in Gitea package registry."
