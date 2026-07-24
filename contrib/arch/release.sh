#!/usr/bin/env bash
#
# Cut a caco release and publish it to the local pacman repo.
#
#   ./contrib/arch/release.sh              # patch bump: 3.3.5 -> 3.3.6
#   ./contrib/arch/release.sh minor        # 3.3.5 -> 3.4.0
#   ./contrib/arch/release.sh 4.0.0        # explicit
#   ./contrib/arch/release.sh --repack     # rebuild this version as pkgrel+1
#
# Flags:
#   --init          create the repo dir, print the pacman.conf stanza, exit
#   --dry-run       show what would happen; change nothing
#   --skip-checks   skip fmt/clippy/test (they are the slow part)
#   --no-push       commit and tag locally, but do not push
#   --no-install    build and publish, but do not run pacman -Syu
#   --repack        rebuild the current version as pkgrel+1 (no version bump)
#
# Env overrides:
#   CACO_PKG_REPO       repo directory   (default /var/lib/pacman-local)
#   CACO_PKG_REPO_NAME  repo/db name     (default local)
#   CACO_PKG_KEEP       builds kept each (default 2)
#   CACO_SWEEP_DAYS     cargo-sweep age  (default 7; 0 disables)
#
# The repo is shared by every locally-built program, not just caco — any other
# project publishes into it the same way, by dropping its packages in and
# re-running repo-add. Only `--init` is caco-specific by accident of living
# here; run it once and no other project needs it.
#
# The whole pipeline is local: nothing is built in CI and no package leaves this
# machine, so GitHub only ever holds source.
#
# Ordering note: the package is BUILT before anything is committed, tagged or
# pushed. A failed build must not leave a published version behind with no
# artifact to match it, so the version files are reverted on any failure before
# the commit step is reached.

set -euo pipefail

REPO_DIR="${CACO_PKG_REPO:-/var/lib/pacman-local}"
REPO_NAME="${CACO_PKG_REPO_NAME:-"pacman-local"}"
KEEP="${CACO_PKG_KEEP:-2}"
SWEEP_DAYS="${CACO_SWEEP_DAYS:-7}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
arch_dir="$root/contrib/arch"

BUMP=patch
DRY=0 SKIP_CHECKS=0 NO_PUSH=0 NO_INSTALL=0 REPACK=0 INIT=0

for a in "$@"; do
    case "$a" in
        --init)         INIT=1 ;;
        --dry-run)      DRY=1 ;;
        --skip-checks)  SKIP_CHECKS=1 ;;
        --no-push)      NO_PUSH=1 ;;
        --no-install)   NO_INSTALL=1 ;;
        --repack)       REPACK=1 ;;
        patch|minor|major)    BUMP="$a" ;;
        [0-9]*.[0-9]*.[0-9]*) BUMP="$a" ;;
        *) echo "unknown argument: $a" >&2; exit 2 ;;
    esac
done

say()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m==> error:\033[0m %s\n' "$*" >&2; exit 1; }
run()  { if (( DRY )); then printf '   would run: %s\n' "$*"; else "$@"; fi; }

# --------------------------------------------------------------------------
# --init: one-time setup of the shared local repo
# --------------------------------------------------------------------------
if (( INIT )); then
    if [[ -d $REPO_DIR && -w $REPO_DIR ]]; then
        say "$REPO_DIR already exists and is writable."
    else
        say "Creating $REPO_DIR owned by $USER (needs sudo once)..."
        sudo install -d -o "$USER" -g "$(id -gn)" -m 755 "$REPO_DIR"
    fi
    cat <<EOF

Register the repo by appending this to /etc/pacman.conf:

    [$REPO_NAME]
    SigLevel = Optional TrustAll
    Server = file://$REPO_DIR

Copy-paste to append it now:

sudo tee -a /etc/pacman.conf >/dev/null <<'PACMANCONF'

[$REPO_NAME]
SigLevel = Optional TrustAll
Server = file://$REPO_DIR
PACMANCONF

Do that AFTER the first release — an empty repo has no database file and
'pacman -Sy' will complain it cannot retrieve $REPO_NAME.db.
EOF
    exit 0
fi

cd "$root"

# --------------------------------------------------------------------------
# Guards — a package is only as trustworthy as the tree it was built from
# --------------------------------------------------------------------------
command -v makepkg  >/dev/null || die "makepkg not found (install base-devel)"
command -v repo-add >/dev/null || die "repo-add not found (install pacman)"
command -v paccache >/dev/null || die "paccache not found (install pacman-contrib)"

[[ -d $REPO_DIR ]] || die "local repo $REPO_DIR does not exist. Run: $0 --init"
[[ -w $REPO_DIR ]] || die "local repo $REPO_DIR is not writable by $USER. Run: $0 --init"

branch="$(git rev-parse --abbrev-ref HEAD)"
[[ $branch == main ]] || die "on branch '$branch'; releases are cut from main"

git diff --quiet && git diff --cached --quiet \
    || die "working tree is dirty — commit or stash first"

say "Fetching origin..."
run git fetch --quiet origin main
if ! (( DRY )); then
    behind="$(git rev-list --count HEAD..origin/main)"
    (( behind == 0 )) || die "local main is $behind commit(s) behind origin/main — pull first"
fi

# --------------------------------------------------------------------------
# Quality gates
# --------------------------------------------------------------------------
if (( SKIP_CHECKS )); then
    warn "Skipping fmt/clippy/test (--skip-checks)"
else
    say "Running quality gates..."
    run cargo fmt --all -- --check
    run cargo clippy --workspace --all-targets -- -D warnings
    run cargo test --workspace
fi

# --------------------------------------------------------------------------
# Work out the new version
# --------------------------------------------------------------------------
OLD="$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)"
[[ -n $OLD ]] || die "could not read workspace version from Cargo.toml"
OLD_RE="${OLD//./\\.}"

if (( REPACK )); then
    NEW="$OLD"
    PKGREL=$(( $(awk -F= '/^pkgrel=/{print $2; exit}' "$arch_dir/PKGBUILD") + 1 ))
    say "Repacking $NEW as pkgrel=$PKGREL (no version bump)"
else
    PKGREL=1
    case "$BUMP" in
        patch|minor|major)
            IFS=. read -r MA MI PA <<<"$OLD"
            case "$BUMP" in
                major) NEW="$((MA + 1)).0.0" ;;
                minor) NEW="$MA.$((MI + 1)).0" ;;
                patch) NEW="$MA.$MI.$((PA + 1))" ;;
            esac
            ;;
        *) NEW="$BUMP" ;;
    esac
    [[ $NEW != "$OLD" ]] || die "new version equals current ($OLD); use --repack to rebuild"

    if git rev-parse -q --verify "refs/tags/v$NEW" >/dev/null; then
        die "tag v$NEW already exists"
    fi
    say "Version $OLD -> $NEW"
fi

# --------------------------------------------------------------------------
# Rewrite the version files
#
# Anything that fails from here until the commit must leave the tree exactly as
# it was found, so the edits are trapped and reverted.
# --------------------------------------------------------------------------
TOUCHED=0
revert_on_failure() {
    local rc=$?
    if (( rc != 0 && TOUCHED )); then
        warn "Failed — reverting version changes to leave the tree clean."
        git checkout -- Cargo.toml Cargo.lock "$arch_dir/PKGBUILD" 2>/dev/null || true
    fi
    exit $rc
}
trap revert_on_failure EXIT

if (( DRY )); then
    printf '   would set version to %s-%s in Cargo.toml, Cargo.lock, PKGBUILD\n' "$NEW" "$PKGREL"
else
    TOUCHED=1
    if ! (( REPACK )); then
        # Two kinds of version reference live in Cargo.toml: the workspace
        # version, and the `version =` field on each internal path dependency.
        # Both must move together or cargo refuses to package the workspace.
        sed -i -E "s/^version = \"$OLD_RE\"$/version = \"$NEW\"/" Cargo.toml
        sed -i -E "s/(caco-(core|sources|cli|gui|tui|mcp) = \{ path = \"[^\"]+\", version = )\"$OLD_RE\"/\1\"$NEW\"/" Cargo.toml

        left="$(grep -c "\"$OLD_RE\"" Cargo.toml || true)"
        (( left == 0 )) || die "Cargo.toml still references $OLD in $left place(s)"

        # Cargo.lock records the workspace members' own versions, so it must be
        # refreshed or the PKGBUILD's --locked build will refuse to run.
        cargo update --workspace --offline --quiet
    fi

    # Keep the literal pkgver/pkgrel in the PKGBUILD in step with Cargo.toml.
    # pkgver() would derive the same value, but committing it stops makepkg
    # from rewriting the file into a surprise diff later. pacman also only
    # treats a rebuild as an upgrade if pkgrel actually moves.
    sed -i -E "s/^pkgver=.*/pkgver=$NEW/"   "$arch_dir/PKGBUILD"
    sed -i -E "s/^pkgrel=.*/pkgrel=$PKGREL/" "$arch_dir/PKGBUILD"
fi

# --------------------------------------------------------------------------
# Build — before any commit, so a failure publishes nothing
#
# --nocheck because the gates above already ran the suite; -f to overwrite an
# existing tarball; -d to skip dependency checks (the Rust toolchain is
# rustup-managed and invisible to pacman); -c to drop src/ and pkg/ afterwards.
# --------------------------------------------------------------------------
say "Building packages (reusing $root/target)..."
run env -C "$arch_dir" makepkg -f -d -c --nocheck --noconfirm

if (( DRY )); then
    trap - EXIT
    say "Dry run complete — nothing was changed."
    exit 0
fi

# --------------------------------------------------------------------------
# Commit, tag, push — the build succeeded, so this version is real
# --------------------------------------------------------------------------
if (( REPACK )); then
    say "Committing repack v$NEW-$PKGREL..."
    git add "$arch_dir/PKGBUILD"
    git commit -q -m "chore(release): repack v$NEW-$PKGREL"
else
    say "Committing and tagging v$NEW..."
    git add Cargo.toml Cargo.lock "$arch_dir/PKGBUILD"
    git commit -q -m "chore(release): v$NEW"
    git tag -a "v$NEW" -m "v$NEW"
fi
TOUCHED=0
trap - EXIT

if (( NO_PUSH )); then
    warn "Not pushed (--no-push). Later: git push origin main"
    (( REPACK )) || warn "  and: git push origin v$NEW"
else
    git push -q origin main
    (( REPACK )) || git push -q origin "v$NEW"
fi

# --------------------------------------------------------------------------
# Publish into the shared local repo
# --------------------------------------------------------------------------
say "Publishing to $REPO_DIR..."
shopt -s nullglob
built=("$arch_dir"/*.pkg.tar.zst)
(( ${#built[@]} )) || die "makepkg produced no packages"
mv -f "${built[@]}" "$REPO_DIR/"

# Retention. paccache already understands package filenames, so it keeps the
# newest $KEEP of each package without confusing caco-gui for a build of caco,
# and it orders by version rather than mtime — so rebuilding an old version
# cannot evict a newer one. It leaves the .db/.files entries alone.
if (( KEEP > 0 )); then
    paccache -r -k "$KEEP" -c "$REPO_DIR" >/dev/null 2>&1 || true
fi

# Rebuild the db from what survived rather than adding incrementally, so it can
# never reference a package file that retention just deleted.
#
# Two safeguards, both learned the hard way. repo-add exits non-zero on any
# unreadable file, and a stray or half-copied .pkg.tar.zst from some other
# project's interrupted build is enough to trigger it — so validate first and
# skip junk rather than letting one bad file abort the rebuild. And build the
# new database in a temp dir, swapping it in only once repo-add has succeeded:
# deleting the old db up front means a failure here leaves the repo with *no*
# database, which breaks pacman -Syu for every program in it, not just caco.
valid=()
for f in "$REPO_DIR"/*.pkg.tar.*; do
    [[ $f == *.sig ]] && continue
    if bsdtar -tqf "$f" .PKGINFO >/dev/null 2>&1; then
        valid+=("$f")
    else
        warn "not a readable package, skipping: $(basename "$f")"
    fi
done
(( ${#valid[@]} )) || die "no valid packages in $REPO_DIR — database left untouched"

tmpdb="$(mktemp -d)"
trap 'rm -rf "$tmpdb"' EXIT
repo-add --quiet "$tmpdb/$REPO_NAME.db.tar.gz" "${valid[@]}"
rm -f "$REPO_DIR/$REPO_NAME".db* "$REPO_DIR/$REPO_NAME".files*
mv -f "$tmpdb/$REPO_NAME".db* "$tmpdb/$REPO_NAME".files* "$REPO_DIR/"
rm -rf "$tmpdb"
trap - EXIT

# --------------------------------------------------------------------------
# Reclaim space
#
# --time rather than --installed or --maxsize: --installed reclaims almost
# nothing on a single-toolchain setup, and --maxsize would evict the warm cache
# this whole design exists to preserve. Age-based pruning clears genuinely stale
# artifacts while leaving the current build intact.
# --------------------------------------------------------------------------
if (( SWEEP_DAYS > 0 )) && command -v cargo-sweep >/dev/null; then
    say "Sweeping cargo artifacts older than ${SWEEP_DAYS}d..."
    cargo sweep --time "$SWEEP_DAYS" "$root" >/dev/null 2>&1 || true
fi

say "Released ${NEW}-${PKGREL}"

if (( NO_INSTALL )); then
    say "Skipping install (--no-install). Run: sudo pacman -Syu"
else
    say "Upgrading..."
    sudo pacman -Syu
fi
