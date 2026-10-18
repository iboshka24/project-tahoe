#!/usr/bin/env bash
# SPDX-License-Identifier: 0BSD
# Build the Tahoe live ISO.
#
#   sudo ./iso/build.sh            (or run inside the CI container, as root)
#   TAHOE_OUT_DIR=/data/iso ./iso/build.sh
#
# What it does, in order:
#   1. builds every first-party package in packages/ (the [tahoe] repo
#      entries referenced by packages.x86_64) with makepkg as an unprivileged
#      user, and registers them in a local file:// repository;
#   2. generates a build-time pacman.conf where [tahoe] points at that
#      repository and every needed mirror list exists;
#   3. runs mkarchiso on the profile.
#
# STATUS: written 2026-10-18, not yet executed. Treat a first run as the
# experiment it is and read mkarchiso's errors calmly; see docs/iso-session.md
# for the honest checklist of what still needs verifying on real hardware.
set -euo pipefail

cd "$(dirname "$0")/.."          # repository root

REPO_DIR="${TAHOE_REPO_DIR:-/tmp/tahoe-repo}"
WORK_DIR="${TAHOE_WORK_DIR:-/tmp/tahoe-work}"
OUT_DIR="${TAHOE_OUT_DIR:-/tmp/tahoe-out}"
BUILD_USER="${TAHOE_BUILD_USER:-tahoe-build}"

say()  { printf '\033[1m== %s ==\033[0m\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$EUID" == 0 ]] || die "run as root inside a clean Arch chroot (see docs/iso-session.md)"

# ── 1. first-party packages ────────────────────────────────────────────────
say "building the [tahoe] repository from packages/"

if ! id -u "$BUILD_USER" >/dev/null 2>&1; then
    useradd -m "$BUILD_USER"
fi
# makepkg refuses to run as root; it also refuses to write into a dir not
# owned by the build user. packages/ stays untouched: builds go to $REPO_DIR.
pacman -S --needed --noconfirm archiso git

# makepkg installs missing dependencies itself (via sudo), but sudo cannot
# prompt on a headless runner — root installs everything first, reading the
# depends/makedepends arrays out of every PKGBUILD in the repo. The PKGBUILDs
# are our own declaration-only files: sourcing them in a subshell is safe.
DEPS=()
for pkgbuild in packages/*/PKGBUILD; do
    while read -r d; do
        [[ -n "$d" ]] && DEPS+=("$d")
    done < <( (source "$pkgbuild" >/dev/null 2>&1; printf '%s\n' "${depends[@]:-}" "${makedepends[@]:-}") )
done
if [[ "${#DEPS[@]}" -gt 0 ]]; then
    # pacman dislikes repeated targets; the PKGBUILDs legitimately repeat
    # makedepends (cargo, git, gcc-libs), so keep the first occurrence only.
    declare -A _seen=()
    UNIQUE=()
    for d in "${DEPS[@]}"; do
        if [[ -z "${_seen[$d]:-}" ]]; then
            _seen[$d]=1
            UNIQUE+=("$d")
        fi
    done
    say "installing first-party PKGBUILD dependencies as root"
    pacman -Syu --needed --noconfirm -- "${UNIQUE[@]}" || die "pacman refused the first-party dependencies"
fi

mkdir -p "$REPO_DIR"
chown -R "$BUILD_USER:$BUILD_USER" "$REPO_DIR"

SKIPPED_PKG=()
shopt -s nullglob
while read -r dir; do
    name="$(basename "$dir")"
    say "building $name"
    SRC_DIR="$dir"
    if (
        mkdir -p "$REPO_DIR/$name"
        cp -a "$SRC_DIR/." "$REPO_DIR/$name/"
        chown -R "$BUILD_USER:$BUILD_USER" "$REPO_DIR/$name"
        cd "$REPO_DIR/$name"
        # pkgver comes from a git tag on GitHub; packages that do not have
        # their tag published yet are skipped with a warning, not fatal.
        # The && keeps this subshell's exit status truthful: a swallowed
        # cp -v … || true used to make the whole build "succeed".
        runuser -u "$BUILD_USER" -- makepkg -Csf --noconfirm --noprogressbar &&
            cp -v ./*.pkg.tar.* "$REPO_DIR/"
    ); then : # built
    else
        warn "could not build $name — its package will be missing from the ISO"
        SKIPPED_PKG+=("$name")
    fi
done < <(find packages -mindepth 1 -maxdepth 1 -type d)

if [[ "${#SKIPPED_PKG[@]}" -gt 0 ]]; then
    warn "skipped packages: ${SKIPPED_PKG[*]}"
    warn "the ISO will boot anyway; the [tahoe] tools are extras, not the desktop"
fi

say "registering the local repository"
PKGS=("$REPO_DIR"/*.pkg.tar.*)
if [[ "${#PKGS[@]}" -gt 0 ]]; then
    repo-add "$REPO_DIR/tahoe.db.tar.zst" "${PKGS[@]}" >/dev/null
else
    warn "no [tahoe] packages were built; the ISO proceeds without them"
fi

# ── 2. build-time pacman.conf ──────────────────────────────────────────────
say "preparing the build pacman.conf"
BUILD_PACMAN_CONF="$WORK_DIR/tahoe-build-pacman.conf"
mkdir -p "$WORK_DIR"
sed "s#^Server = https://repo.tahoe.local/\\\$arch#Server = file://$REPO_DIR#" \
    iso/pacman.conf >"$BUILD_PACMAN_CONF"
if [[ "${#PKGS[@]}" -eq 0 ]]; then
    # pacman cannot tolerate a repository section without its database: drop
    # the whole [tahoe] section (header, SigLevel, Server, trailing blank)
    # from the build-time copy so archiso syncs core/extra only.
    sed -i '/^\[tahoe\]/,/^$/d' "$BUILD_PACMAN_CONF"
fi

# ── 3. the ISO ──────────────────────────────────────────────────────────────
say "running mkarchiso"
PROFILE_DIR="iso"
if [[ "${#SKIPPED_PKG[@]}" -gt 0 ]]; then
    # mkarchiso reads <profile>/packages.x86_64 verbatim, so packages that
    # could not be built must come off that list from a sanitized copy —
    # the committed profile stays untouched.
    say "dropping unbuilt packages from a profile copy"
    PROFILE_DIR="$WORK_DIR/profile-iso"
    rm -rf "$PROFILE_DIR"
    mkdir -p "$PROFILE_DIR"
    cp -a iso/. "$PROFILE_DIR/"
    for name in "${SKIPPED_PKG[@]}"; do
        sed -i "/^${name}\$/d" "$PROFILE_DIR/packages.x86_64"
    done
fi
rm -rf "$WORK_DIR/airootfs" "$WORK_DIR/iso"
mkarchiso -C "$BUILD_PACMAN_CONF" -v -w "$WORK_DIR" -o "$OUT_DIR" "$PROFILE_DIR"

say "done"
ISO_PATH="$(find "$OUT_DIR" -maxdepth 2 -name '*.iso' | head -n1)"
echo "ISO: $ISO_PATH"
if [[ "${#SKIPPED_PKG[@]}" -gt 0 ]]; then
    warn "remember: ${SKIPPED_PKG[*]} are missing from this build"
fi
