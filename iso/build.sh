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
# STATUS: first green CI run 2026-10-18 (GitHub run 39159947256): the three
# [tahoe] packages were skipped for their missing v0.1.0 tags, the fail-soft
# path ran end to end, and mkarchiso produced tahoe-2026.10.18-x86_64.iso
# (1.7 GiB) with [extra] packages only. Booting the image is still unverified;
# see docs/iso-session.md for the honest checklist of what remains.
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

# ── 1b. the installer: third-party packages we rebuild ourselves ───────────
#
# Calamares is not in Arch's repositories — the packages API returns nothing
# for it and it is absent from the extra/ listing — so a distribution that
# wants it has to build it, which is exactly what CachyOS does. This is ours.
#
# Everything it depends on *is* in extra and is verified there rather than
# assumed: kcoreaddons 6.31.0, kpmcore 26.08.2, libpwquality 1.4.5,
# qt6-declarative, qt6-svg, yaml-cpp 0.9.0, plus extra-cmake-modules,
# libglvnd, ninja, qt6-tools and qt6-translations to build. So this list is
# one package long; if a dependency ever turns out to be AUR-only too, add it
# here and it will be built first and installed before its dependent.
#
# Every step is fail-soft, like the first-party loop: if the installer cannot
# be built the ISO is still produced without it, and the reason is printed.
say "building the installer from the AUR"
AUR_PKGS=(calamares)
AUR_FAILED=()
AUR_MISSING_DEPS=()

for name in "${AUR_PKGS[@]}"; do
    if [[ " ${AUR_FAILED[*]:-} " == *" $name "* ]]; then
        warn "$name depends on something that failed; skipping"
        AUR_FAILED+=("$name")
        continue
    fi
    # dependents must come after their AUR dependencies in AUR_PKGS: the loop
    # is a single pass, not a solver.

    rm -rf "$REPO_DIR/aur/$name"
    if ! git clone --depth 1 "https://aur.archlinux.org/${name}.git" "$REPO_DIR/aur/$name"; then
        warn "could not clone the AUR package $name"
        AUR_FAILED+=("$name")
        continue
    fi
    chown -R "$BUILD_USER:$BUILD_USER" "$REPO_DIR/aur/$name"

    # depends and makedepends both have to be present before makepkg runs:
    # makepkg's own dependency step goes through sudo, which cannot prompt on
    # a headless runner. Version constraints are stripped — pacman resolves
    # the version, and this tree may legitimately be newer than the floor.
    while read -r dep; do
        [[ -z "$dep" ]] && continue
        [[ " ${AUR_PKGS[*]} " == *" $dep "* ]] && continue
        if ! pacman -S --needed --noconfirm -- "$dep" >/dev/null 2>&1; then
            AUR_MISSING_DEPS+=("$name:$dep")
        fi
    done < <(
        (source "$REPO_DIR/aur/$name/PKGBUILD" >/dev/null 2>&1
         printf '%s\n' "${depends[@]:-}" "${makedepends[@]:-}") \
        | sed 's/[<>=].*//' | sed '/^$/d' | sort -u
    )

    if (cd "$REPO_DIR/aur/$name" &&
            runuser -u "$BUILD_USER" -- makepkg -Csf --noconfirm --noprogressbar &&
            cp ./*.pkg.tar.* "$REPO_DIR/"); then
        # Install it, so the next package in the list can link against it.
        pacman -U --noconfirm "$REPO_DIR/aur/$name"/*.pkg.tar.* >/dev/null ||
            warn "$name built but did not install; a dependent may fail"
        echo "built $name at $(git -C "$REPO_DIR/aur/$name" rev-parse HEAD)"
    else
        warn "could not build $name; the installer will be missing from the ISO"
        AUR_FAILED+=("$name")
    fi
done

if [[ "${#AUR_MISSING_DEPS[@]}" -gt 0 ]]; then
    warn "dependencies no repository would provide: ${AUR_MISSING_DEPS[*]}"
    warn "if any of those is an AUR package too, add it to AUR_PKGS in this script"
fi
if [[ "${#AUR_FAILED[@]}" -gt 0 ]]; then
    warn "installer packages that failed: ${AUR_FAILED[*]}"
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

# A package that no PKGBUILD in packages/ produces cannot be "skipped" by the
# loop above — mkarchiso would fail on it with "target not found". The
# installer is the one entry like that (calamares is AUR-only, see
# docs/iso-installer.md), so it is dropped from the profile copy when the
# local repository does not have it, exactly as unbuilt first-party packages
# are, and the ISO keeps building without it.
if ! compgen -G "$REPO_DIR/calamares-*.pkg.tar.*" >/dev/null; then
    warn "calamares is not in the local repository: the ISO will have no installer"
    SKIPPED_PKG+=("calamares")
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
