#!/usr/bin/env bash
# SPDX-License-Identifier: 0BSD
# Re-vendor MacShell + MacLock from upstream into the ISO profile.
#
#   ./iso/vendor-macshell.sh                  # re-read the pinned commit
#   ./iso/vendor-macshell.sh REF              # a branch, tag or commit
#
# Where it lands:
#   iso/airootfs/etc/skel/.config/quickshell/MacShell/
#   iso/airootfs/etc/skel/.config/quickshell/MacLock/
#
# After updating, refresh the provenance block in iso/macshell-vendor.md
# (commit, date) and re-read the upstream README's keybind/layer-rule block —
# airootfs/etc/skel/.config/hypr/hyprland.lua keeps a copy of it that should
# stay in step.
set -euo pipefail

REPO_URL="${MACSHELL_URL:-https://github.com/0-ss/MacShell.git}"
PINNED_REF="eebaaad667bc847287dc8baeda93dbec97401fa4"
REF="${1:-$PINNED_REF}"
DEST="$(cd "$(dirname "$0")" && pwd)/airootfs/etc/skel/.config/quickshell"

say() { printf '\033[1m== %s ==\033[0m\n' "$*"; }

command -v git >/dev/null || { echo "git is required" >&2; exit 1; }
command -v rsync >/dev/null || { echo "rsync is required" >&2; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

say "cloning $REPO_URL"
git clone "$REPO_URL" "$TMP/macshell"
git -C "$TMP/macshell" checkout --detach "$REF"

for SUB in MacShell MacLock; do
    [[ -d "$TMP/macshell/$SUB" ]] || { echo "upstream tree $SUB is missing; refusing to copy" >&2; exit 1; }
    say "syncing $SUB"
    mkdir -p "$DEST/$SUB"
    rsync -a --delete --exclude .git "$TMP/macshell/$SUB/" "$DEST/$SUB/"
done

say "done — now update iso/macshell-vendor.md with the new commit and date"
