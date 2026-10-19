#!/usr/bin/env bash
# SPDX-License-Identifier: 0BSD
#
# Structural checks for the Tahoe archiso profile (iso/).
#
# Runs anywhere bash runs; no root, no network, no archiso needed. Its job is
# to catch at commit time the mistakes that would otherwise surface minutes
# into a real mkarchiso run: a missing vendored file, a CRLF ending in a
# shell script, a package conflict with MacShell, a Lua call the compositor
# does not actually implement.
#
# Usage: tests/check-iso.sh

set -uo pipefail

cd "$(dirname "$0")/.." || exit 1

pass=0
fail=0
skipped=0

ok()   { printf '  \033[32mok\033[0m    %s\n' "$1"; pass=$((pass + 1)); }
bad()  { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; fail=$((fail + 1)); }
skip() { printf '  \033[33mskip\033[0m  %s\n' "$1"; skipped=$((skipped + 1)); }

# check_if_runs <label> <command...>
check_exists() {
    local label="$1" path="$2"
    if [[ -e "$path" ]]; then ok "$label"
    else bad "$label ($path missing)"; fi
}

check_grep() {
    local label="$1" pattern="$2" file="$3"
    # -e keeps patterns that begin with '-' away from grep's own options,
    # e.g. '--autologin tahoe' for the agetty auto-login drop-in.
    if grep -qE -e "$pattern" "$file" 2>/dev/null; then ok "$label"
    else bad "$label (pattern '$pattern' not found in $file)"; fi
}

LUA="iso/airootfs/etc/skel/.config/hypr/hyprland.lua"
PKG="iso/packages.x86_64"

# ---------------------------------------------------------------------------
printf '\033[1m== ISO profile: files exist ==\033[0m\n'

for f in \
    "$LUA" \
    iso/airootfs/etc/skel/.config/hypr/hyprpaper.conf \
    iso/airootfs/etc/skel/.config/macshell/env \
    iso/airootfs/etc/skel/.config/macdock/settings.json \
    iso/airootfs/etc/skel/.bash_profile \
    iso/airootfs/etc/skel/.local/bin/macshell-start \
    iso/airootfs/etc/skel/.config/quickshell/MacShell/shell.qml \
    iso/airootfs/etc/skel/.config/quickshell/MacShell/MacBar.qml \
    iso/airootfs/etc/skel/.config/quickshell/MacShell/MacDock.qml \
    iso/airootfs/etc/skel/.config/quickshell/MacLock/shell.qml \
    iso/airootfs/etc/skel/.config/quickshell/MacLock/lock-via-ipc.sh \
    iso/airootfs/etc/skel/Pictures/Wallpapers/Tahoe-Dark.png \
    iso/airootfs/etc/skel/Pictures/Wallpapers/Tahoe-Light.png \
    iso/airootfs/etc/passwd \
    iso/airootfs/etc/shadow \
    iso/airootfs/etc/locale.conf \
    iso/airootfs/etc/motd \
    iso/airootfs/etc/sudoers.d/10-tahoe-live \
    iso/airootfs/etc/systemd/system/getty@tty1.service.d/autologin.conf \
    iso/macshell-vendor.md ; do
    check_exists "$f" "$f"
done

# ---------------------------------------------------------------------------
printf '\033[1m== ISO profile: content ==\033[0m\n'

check_grep "passwd declares the live user"          '^(root|tahoe):x?:' iso/airootfs/etc/passwd
check_grep "tahoe uid is 1000 with /home/tahoe"     '^tahoe:x:1000:1000:.*:/home/tahoe:/bin/bash$' iso/airootfs/etc/passwd
check_grep "passwordless login hashes"              '^root::[0-9]+:' iso/airootfs/etc/shadow
check_grep "tty1 auto-login as tahoe"               '--autologin tahoe' iso/airootfs/etc/systemd/system/getty@tty1.service.d/autologin.conf
check_grep "the session starts on tty1"             'exec /usr/bin/Hyprland' "iso/airootfs/etc/skel/.bash_profile"
check_grep "MacShell autostart present"             'macshell-start' "$LUA"
check_grep "the MacShell markers survived"          '>>> MacShell' "$LUA"
check_grep "Liquid Glass blur requested"            'name = "macdock".*blur = true' "$LUA"
check_grep "session-lock restore for MacLock"       'allow_session_lock_restore' "$LUA"
check_grep "wallpaper is served by hyprpaper"       '^preload=' "iso/airootfs/etc/skel/.config/hypr/hyprpaper.conf"

# the vendored start script must be the upstream one by name and shape
check_grep "macshell-start waits for the socket"    'WAYLAND_DISPLAY' iso/airootfs/etc/skel/.local/bin/macshell-start
check_grep "macshell-start restarts on crash"       'fast=0' iso/airootfs/etc/skel/.local/bin/macshell-start

# wallpaper must be a real PNG (8-byte magic), wherever the shell expects it
PNG_OK=0
while IFS= read -r png; do
    if python3 - "$png" <<'PY' 2>/dev/null
import sys
sys.exit(0 if open(sys.argv[1], "rb").read(8) == b"\x89PNG\r\n\x1a\n" else 1)
PY
    then PNG_OK=$((PNG_OK + 1)); fi
done < <(find iso/airootfs/etc/skel/Pictures/Wallpapers -name '*.png' 2>/dev/null)
if [[ "${PNG_OK:-0}" -eq 2 ]]; then ok "both shipped wallpapers are real PNGs"
else bad "shipped wallpapers missing or not PNG ($PNG_OK/2 valid)"; fi

# ---------------------------------------------------------------------------
printf '\033[1m== Hyprland config: Lua API sanity ==\033[0m\n'

# every dispatcher the config uses must be one Hyprland 0.55 documents
DSP_WHITELIST=(
    hl\.dsp\.exec_cmd
    hl\.dsp\.window\.close
    hl\.dsp\.window\.float
    hl\.dsp\.window\.fullscreen
    hl\.dsp\.window\.center
    hl\.dsp\.window\.move
    hl\.dsp\.focus
    hl\.dsp\.reload_config
)
USED=$(grep -oE 'hl\.dsp\.[A-Za-z_.]+' "$LUA" | sort -u)
UNUSED_FAIL=0
while IFS= read -r call; do
    [[ -z "$call" ]] && continue
    m=0
    for w in "${DSP_WHITELIST[@]}"; do
        [[ "$call" =~ ^$w$ ]] && m=1 && break
    done
    if [[ "$m" == 0 ]]; then
        bad "unknown dispatcher: $call"
        UNUSED_FAIL=1
    fi
done <<< "$USED"
[[ "$UNUSED_FAIL" == 0 ]] && ok "all $(<<<"$USED" grep -c .) dispatcher calls are documented Hyprland 0.55 API"

# hyprlang leftovers must not exist in a Lua config
if grep -qE '^(bind|exec-once|env|monitor|exec)[[:space:]]*=' "$LUA"; then
    bad "hyprlang syntax inside the .lua config (bind=/exec-once=/env=)"
else
    ok "no hyprlang syntax stayed behind"
fi

# the four upstream layer rules must be intact
for ns in macdock macspot macbarbg macshot; do
    check_grep "layer rule for $ns" "namespace = \"$ns\"" "$LUA"
done

# ---------------------------------------------------------------------------
printf '\033[1m== Package list ==\033[0m\n'

for p in hyprland quickshell qt6-shadertools qt6-svg hyprlock hyprpaper \
         pipewire wireplumber networkmanager kitty papirus-icon-theme; do
    check_grep "packages list has $p" "^$p$" "$PKG"
done

# MacShell REPLACES these daemons; their presence is a conscious conflict
CONFLICTS=(waybar hyprpanel dunst mako swaync nwg-dock ags)
CONFLICT_FOUND=0
for p in "${CONFLICTS[@]}"; do
    if grep -qE "^$p$" "$PKG"; then
        bad "conflict with MacShell: $p is in the package list"
        CONFLICT_FOUND=1
    fi
done
if [[ "$CONFLICT_FOUND" == 0 ]]; then ok "no bars/docks/notification daemons conflict with MacShell"; fi

# ---------------------------------------------------------------------------
printf '\033[1m== Syntax of shipped scripts ==\033[0m\n'

sh -n iso/airootfs/etc/skel/.local/bin/macshell-start 2>/dev/null &&
    ok "macshell-start parses (sh)" || bad "macshell-start has a syntax error"
bash -n iso/build.sh 2>/dev/null &&
    ok "build.sh parses" || bad "build.sh has a syntax error"
bash -n iso/vendor-macshell.sh 2>/dev/null &&
    ok "vendor-macshell.sh parses" || bad "vendor-macshell.sh has a syntax error"
bash -n iso/profiledef.sh 2>/dev/null &&
    ok "profiledef.sh parses" || bad "profiledef.sh has a syntax error"

if python3 -m json.tool iso/airootfs/etc/skel/.config/macdock/settings.json >/dev/null 2>&1; then
    ok "macdock settings.json is valid JSON"
else
    bad "macdock settings.json is not JSON"
fi

# ---------------------------------------------------------------------------
printf '\033[1m== Line endings and git modes ==\033[0m\n'

# CR-counting helper: od dumps raw bytes, awk counts the '\r' tokens.
# (grep is used everywhere else for ERE matches; here it cannot: MSYS
# builds of grep and of cat-file|grep pipes rewrite line endings on the
# fly in some Windows environments, so their verdicts about CR bytes are
# not trustworthy. od reads through no text-mode layer.)
cr_count() {
    od -An -c "$1" 2>/dev/null | awk 'BEGIN { c = 0 } { c += gsub(/\\r/, "x") } END { print c + 0 }'
}

CRLF_FILES=()
for f in \
    "$LUA" \
    iso/airootfs/etc/skel/.bash_profile \
    iso/airootfs/etc/skel/.config/macshell/env \
    iso/airootfs/etc/skel/.config/hypr/hyprpaper.conf \
    iso/airootfs/etc/skel/.local/bin/macshell-start \
    iso/airootfs/etc/passwd \
    iso/airootfs/etc/shadow \
    iso/airootfs/etc/sudoers.d/10-tahoe-live \
    iso/airootfs/etc/systemd/system/getty@tty1.service.d/autologin.conf ; do
    if [ "$(cr_count "$f")" -gt 0 ]; then
        CRLF_FILES+=("$f")
    fi
done
if [[ "${#CRLF_FILES[@]}" -eq 0 ]]; then
    ok "no CRLF endings in the files that must stay POSIX"
else
    bad "CRLF endings found in: ${CRLF_FILES[*]}"
fi

if [[ -d .git ]]; then
    MODE=$(git ls-files -s iso/airootfs/etc/skel/.local/bin/macshell-start | awk '{print $1}')
    if [[ "$MODE" == "100755" ]]; then ok "macshell-start has the executable bit"
    else bad "macshell-start should be git mode 100755, is '$MODE'"; fi

    for s in build.sh vendor-macshell.sh; do
        MODE=$(git ls-files -s "iso/$s" | awk '{print $1}')
        if [[ "$MODE" == "100755" ]]; then ok "iso/$s has the executable bit"
        else bad "iso/$s should be git mode 100755, is '$MODE'"; fi
    done

    WANTS=$(git ls-files -s iso/airootfs/etc/systemd/system/multi-user.target.wants |
        grep -c '^120000' || true)
    if [[ "$WANTS" -eq 2 ]]; then ok "both service .wants symlinks are recorded as symlinks (120000)"
    else bad "multi-user.target.wants should contain exactly 2 symlinks, git sees $WANTS"; fi
else
    skip "git metadata (not a checkout)"
fi

# vendor consistency: pin in the script must equal pin in the provenance doc
PIN_SH=$(grep -oP '^PINNED_REF="\K[0-9a-f]{40}' iso/vendor-macshell.sh 2>/dev/null || echo)
PIN_DOC=$(grep -oP 'Vendored commit \| `\K[0-9a-f]{40}' iso/macshell-vendor.md 2>/dev/null || echo)
if [[ -n "$PIN_SH" && "$PIN_SH" == "$PIN_DOC" ]]; then
    ok "vendor pin is consistent (script ↔ provenance doc)"
else
    bad "vendor pin mismatch: script='${PIN_SH:-?}' doc='${PIN_DOC:-?}'"
fi

# ---------------------------------------------------------------------------
printf '\033[1m== Kernel command line ==\033[0m\n'
# ---------------------------------------------------------------------------
#
# The parameters on the kernel command line are not free-form: the archiso
# hook reads a specific set, and a name it does not know is *silently
# ignored*, after which the initramfs cannot find its own medium and drops to
# "ERROR: '' device did not show up after 30 seconds". The built ISO shipped
# `img_label=`, which belongs to the archiso_loop_mnt hook (booting from an
# ISO file, and it needs img_loop= too) — so nothing read it and the image did
# not boot on any machine, which no structural check noticed until one booted
# it. This is that check.

for cfg in iso/syslinux/syslinux.cfg iso/efiboot/loader/entries/tahoe.conf; do
    if grep -qE '(^|[[:space:]])(APPEND|options)[[:space:]].*archisolabel=' "$cfg"; then
        ok "$(basename "$cfg") passes archisolabel (read by the archiso hook)"
    else
        bad "$(basename "$cfg") does not pass archisolabel — the archiso hook will not find the medium"
    fi
    if grep -qE '[[:space:]]img_label=' "$cfg"; then
        bad "$(basename "$cfg") passes img_label, which only the loop-mount hook reads"
    else
        ok "$(basename "$cfg") does not pass img_label"
    fi
done

if grep -q 'systemd.firstboot=no' iso/syslinux/syslinux.cfg &&
   grep -q 'systemd.firstboot=no' iso/efiboot/loader/entries/tahoe.conf; then
    ok "both entries suppress the first-boot prompt (else the desktop never starts)"
else
    bad "systemd.firstboot=no is missing from a boot entry"
fi

# ---------------------------------------------------------------------------
printf '\n\033[1m%d passed, %d failed, %d skipped\033[0m\n' \
    "$pass" "${fail}" "${skipped:-0}"
[[ "$fail" -eq 0 ]] || exit 1
