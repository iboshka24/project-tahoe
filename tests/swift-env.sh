#!/usr/bin/env bash
# SPDX-License-Identifier: 0BSD
#
# Install a Swift toolchain for Apple development, without root.
#
# Arch ships no Swift. xtool needs Swift **6.4**; swift.org publishes tarballs
# for several distributions and none of them is Arch. This script fetches the
# Ubuntu build into a user directory and patches the one interoperability
# problem Arch has, explained below.
#
# Nothing here uses sudo. Everything lands under $HOME and can be removed by
# deleting the two directories it creates.
#
# Usage:
#   tests/swift-env.sh              # install (or verify) the default version
#   tests/swift-env.sh 6.5.0        # a different release
#   tests/swift-env.sh --verify     # report what is installed, change nothing
#   tests/swift-env.sh --remove     # delete the toolchain and the shim
#
# After it runs, put this on your PATH:
#     export PATH="$HOME/.local/share/swift-toolchain/usr/bin:$PATH"

set -euo pipefail

VERSION="${1:-6.4.1}"
ROOT="${TAHOE_SWIFT_ROOT:-$HOME/.local/share/swift-toolchain}"
DOWNLOAD_DIR="${TAHOE_SWIFT_DOWNLOAD:-$HOME/.swift-download}"

# Why an Ubuntu build on Arch
# ---------------------------
# swift.org publishes Ubuntu, Debian, Fedora, RHEL and Amazon Linux builds.
# Arch's glibc is newer than all of them, and glibc is forward-compatible, so
# any of them runs. Ubuntu 26.04 was chosen because its glibc is the closest
# to Arch's, which minimises the chance of tripping an old-symbol assumption.
OS_SLUG="ubuntu26.04"
DIR_SLUG="ubuntu2604"

# The one thing that does not work out of the box
# ----------------------------------------------
# The Ubuntu build links `libncurses.so.6`. Arch builds ncurses wide-only and
# ships `libncursesw.so.6`, so `swiftc` fails with
#
#     swift: error while loading shared libraries: libncurses.so.6
#
# `libncursesw` exports the narrow API as well, so a symlink satisfies it.
# The symlink is placed inside the toolchain's own `lib/swift/linux`, which
# is already on `swiftc`'s RUNPATH — so no `LD_LIBRARY_PATH` is needed, and
# that matters, because SwiftPM spawns `swiftc` for linking without
# propagating the caller's environment. Exporting the variable fixes
# `swift --version` and then fails at link time, which is a confusing way to
# spend an afternoon.
NARROW_LIB="libncurses.so.6"
WIDE_LIB="/usr/lib/libncursesw.so.6"

info() { printf '\033[1m==\033[0m %s\n' "$1"; }
note() { printf '   %s\n' "$1"; }

verify() {
    local swift="$ROOT/usr/bin/swift"
    if [[ ! -x "$swift" ]]; then
        echo "no Swift toolchain at $ROOT"
        return 1
    fi
    echo "toolchain: $ROOT"
    "$swift" --version | head -2 | sed 's/^/  /'
    local shim="$ROOT/usr/lib/swift/linux/$NARROW_LIB"
    if [[ -L "$shim" || -f "$shim" ]]; then
        echo "ncurses shim: $shim -> $(readlink -f "$shim" 2>/dev/null || echo '(not a link)')"
    else
        echo "ncurses shim: MISSING (swiftc will fail to load)"
        return 1
    fi
    echo "PATH entry:   $ROOT/usr/bin"
    return 0
}

case "${1:-}" in
    --verify)
        verify
        exit $?
        ;;
    --remove)
        info "removing"
        rm -rf "$ROOT"
        note "removed $ROOT"
        note "(the download cache in $DOWNLOAD_DIR was left alone)"
        exit 0
        ;;
    --help | -h)
        sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
esac

TAG="swift-${VERSION}-RELEASE"
URL="https://download.swift.org/swift-${VERSION}-release/${DIR_SLUG}/${TAG}/${TAG}-${OS_SLUG}.tar.gz"
TARBALL="$DOWNLOAD_DIR/${TAG}-${OS_SLUG}.tar.gz"

info "Swift ${VERSION} for Apple development"
note "toolchain goes to $ROOT"
note "download goes to  $TARBALL"
echo

if [[ -x "$ROOT/usr/bin/swift" ]] && "$ROOT/usr/bin/swift" --version >/dev/null 2>&1; then
    info "already installed and working"
    verify
    exit 0
fi

mkdir -p "$DOWNLOAD_DIR" "$ROOT"

if [[ ! -s "$TARBALL" ]]; then
    info "downloading (about 1.1 GB)"
    # `-C -` resumes; the tarball is large enough that a dropped connection
    # is worth surviving.
    curl -fSL --retry 3 -C - -o "$TARBALL" "$URL"
else
    info "using cached archive"
fi

info "extracting"
note "about 3.6 GB; this takes a minute"
tar xzf "$TARBALL" --strip-components=1 -C "$ROOT"

info "installing the ncurses shim"
if [[ ! -e "$WIDE_LIB" ]]; then
    echo "error: $WIDE_LIB does not exist; this script expected Arch's wide-only ncurses" >&2
    exit 1
fi
SHIM_DIR="$ROOT/usr/lib/swift/linux"
mkdir -p "$SHIM_DIR"
ln -sf "$WIDE_LIB" "$SHIM_DIR/$NARROW_LIB"
note "$SHIM_DIR/$NARROW_LIB -> $WIDE_LIB"

info "verifying"
if ! "$ROOT/usr/bin/swift" --version >/dev/null 2>&1; then
    echo "error: the toolchain does not run. Check that $WIDE_LIB exists." >&2
    exit 1
fi
verify

echo
info "done"
note "add this to your shell profile:"
note "    export PATH=\"\$HOME/.local/share/swift-toolchain/usr/bin:\$PATH\""
note ""
note "then verify with:"
note "    tests/swift-env.sh --verify"
note "    tests/check-all.sh          # will now include the Swift checks"
echo
note "still missing for a full iOS build: an Xcode.xip, which requires an"
note "Apple ID. See docs/apple-development.md."
