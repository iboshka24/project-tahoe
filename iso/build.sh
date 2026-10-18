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
# First-party makedepends (read from the PKGBUILDs): cargo + git.
pacman -S --needed --noconfirm archiso cargo git

mkdir -p "$REPO_DIR"
chown -R "$BUILD_USER:$BUILD_USER" "$REPO_DIR"

SKIPPED_PKG=()
while read -r dir; do
    name="$(basename "$dir")"
    say "building $name"
    SRC_DIR="$dir"
    (
        mkdir -p "$REPO_DIR/$name"
        cp -a "$SRC_DIR/." "$REPO_DIR/$name/"
        chown -R "$BUILD_USER:$BUILD_USER" "$REPO_DIR/$name"
        cd "$REPO_DIR/$name"
        # pkgver comes from a git tag on GitHub; packages that do not have
        # their tag published yet are skipped with a warning, not fatal.
        runuser -u "$BUILD_USER" -- makepkg -Csf --noconfirm --noprogressbar
        cp -v ./*.pkg.tar.* "$REPO_DIR/" 2>/dev/null || true
    ) || { warn "could not build $name — its package will be missing from the ISO"; SKIPPED_PKG+=("$name"); }
done < <(find packages -mindepth 1 -maxdepth 1 -type d)

if [[ "${#SKIPPED_PKG[@]}" -gt 0 ]]; then
    warn "skipped packages: ${SKIPPED_PKG[*]}"
    warn "the ISO will boot anyway; the [tahoe] tools are extras, not the desktop"
fi

say "registering the local repository"
repo-add "$REPO_DIR/tahoe.db.tar.zst" "$REPO_DIR"/*.pkg.tar.* >/dev/null

# ── 2. build-time pacman.conf ──────────────────────────────────────────────
say "preparing the build pacman.conf"
BUILD_PACMAN_CONF="$WORK_DIR/tahoe-build-pacman.conf"
mkdir -p "$WORK_DIR"
sed "s#^Server = https://repo.tahoe.local/\\\$arch#Server = file://$REPO_DIR#" \
    iso/pacman.conf >"$BUILD_PACMAN_CONF"

# ── 3. the ISO ──────────────────────────────────────────────────────────────
say "running mkarchiso"
rm -rf "$WORK_DIR/airootfs" "$WORK_DIR/iso"
mkarchiso -C "$BUILD_PACMAN_CONF" -v -w "$WORK_DIR" -o "$OUT_DIR" iso

say "done"
ISO_PATH="$(find "$OUT_DIR" -maxdepth 2 -name '*.iso' | head -n1)"
echo "ISO: $ISO_PATH"
if [[ "${#SKIPPED_PKG[@]}" -gt 0 ]]; then
    warn "remember: ${SKIPPED_PKG[*]} are missing from this build"
fi
