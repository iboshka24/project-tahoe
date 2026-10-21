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

check_not_grep() {
    local label="$1" pattern="$2" file="$3"
    if grep -qE -e "$pattern" "$file" 2>/dev/null; then
        bad "$label (pattern '$pattern' unexpectedly found in $file)"
    else ok "$label"; fi
}

LUA="iso/airootfs/etc/skel/.config/hypr/hyprland.lua"
PKG="iso/packages.x86_64"

# ---------------------------------------------------------------------------
printf '\033[1m== ISO profile: files exist ==\033[0m\n'

for f in \
    "$LUA" \
    iso/airootfs/etc/skel/.config/macshell/env \
    iso/airootfs/etc/skel/.config/macshell/vendor \
    iso/airootfs/etc/skel/.config/macdock/settings.json \
    iso/airootfs/etc/skel/.config/macwidgets/settings.json \
    iso/airootfs/etc/skel/.bash_profile \
    iso/airootfs/etc/skel/.local/bin/macshell-start \
    iso/airootfs/usr/local/bin/tahoe-installer \
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
    iso/airootfs/etc/calamares/settings.conf \
    iso/airootfs/etc/calamares/branding/tahoe/branding.desc \
    iso/airootfs/etc/calamares/branding/tahoe/stylesheet.qss \
    iso/airootfs/etc/calamares/branding/tahoe/show.qml \
    iso/airootfs/etc/calamares/branding/tahoe/logo.svg \
    iso/airootfs/etc/calamares/modules/unpackfs.conf \
    iso/airootfs/usr/share/applications/tahoe-install.desktop \
    iso/airootfs/usr/local/bin/tahoe-installer \
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
check_grep "wallpaper is served by swaybg"          'env WLR_RENDERER=pixman swaybg -i /home/tahoe/Pictures/Wallpapers/Tahoe-Dark.png -m fill' "$LUA"
check_not_grep "hyprpaper is not autostarted"        'exec_cmd\("hyprpaper"\)' "$LUA"
check_grep "the wallpaper picker uses swaybg"        'setsid swaybg' \
    iso/airootfs/etc/skel/.config/quickshell/MacShell/MacBar.qml
check_exists "the pixman swaybg wrapper ships"        iso/airootfs/usr/local/bin/swaybg
check_grep "the wrapper defaults to pixman"           'WLR_RENDERER=pixman' iso/airootfs/usr/local/bin/swaybg
check_grep "the wrapper is executable in the image"   '/usr/local/bin/swaybg' iso/profiledef.sh

# The installed home must identify the MacShell pin, and the first-run
# layout must exercise all four upstream widget types.
check_grep "installed MacShell version marker"       'MACSHELL_VERSION=0.2.8' \
    iso/airootfs/etc/skel/.config/macshell/vendor
check_grep "installed MacShell commit marker"        'MACSHELL_COMMIT=eebaaad667bc847287dc8baeda93dbec97401fa4' \
    iso/airootfs/etc/skel/.config/macshell/vendor
check_grep "first-run Clock widget"                  '"type": "clock"' \
    iso/airootfs/etc/skel/.config/macwidgets/settings.json
check_grep "first-run Calendar widget"               '"type": "calendar"' \
    iso/airootfs/etc/skel/.config/macwidgets/settings.json
check_grep "first-run Weather widget"                '"type": "weather"' \
    iso/airootfs/etc/skel/.config/macwidgets/settings.json
check_grep "first-run Player widget"                 '"type": "player"' \
    iso/airootfs/etc/skel/.config/macwidgets/settings.json

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

for p in hyprland quickshell qt6-shadertools qt6-svg qt6-multimedia \
         hyprlock swaybg jq curl playerctl \
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
sh -n iso/airootfs/usr/local/bin/swaybg 2>/dev/null &&
    ok "the swaybg wrapper parses (sh)" || bad "the swaybg wrapper has a syntax error"
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
if python3 -m json.tool iso/airootfs/etc/skel/.config/macwidgets/settings.json >/dev/null 2>&1; then
    ok "macwidgets settings.json is valid JSON"
else
    bad "macwidgets settings.json is not JSON"
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
    iso/airootfs/etc/skel/.local/bin/macshell-start \
    iso/airootfs/usr/local/bin/tahoe-installer \
    iso/airootfs/usr/local/bin/swaybg \
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

    MODE=$(git ls-files -s iso/airootfs/usr/local/bin/swaybg | awk '{print $1}')
    if [[ "$MODE" == "100755" ]]; then ok "the swaybg wrapper has the executable bit"
    else bad "the swaybg wrapper should be git mode 100755, is '$MODE'"; fi

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
printf '\033[1m== Installer (Calamares) ==\033[0m\n'
# ---------------------------------------------------------------------------
#
# The installer is Calamares with Tahoe branding. Calamares is AUR-only, so
# the ISO can legitimately be built without it — but the branding and the
# launcher are ours and must not drift, and the launch keybind is the only
# documented way to reach the installer in the live session.

CA=iso/airootfs/etc/calamares
check_grep "branding names the product"      '^    productName:.*Project Tahoe' "$CA/branding/tahoe/branding.desc"
check_grep "branding uses the accent token"  '#0A84FF' "$CA/branding/tahoe/stylesheet.qss"
check_grep "settings.conf selects the branding" '^branding: tahoe' "$CA/settings.conf"

# Every module in the sequence has to exist, or Calamares refuses to start:
#   ERROR: Module "mkinitcpio" not found in module search paths.
# and shows "Calamares Initialization Failed" instead of the installer.
# This list is the module directory of the calamares-3.4.2 tarball, which is
# the version iso/build.sh builds; a module renamed upstream fails here.
known_modules="bootloader contextualprocess displaymanager dracut dracutlukscfg
dummycpp dummyprocess dummypython finished finishedq fsresizer fstab grubcfg
hostinfo hwclock initcpio initcpiocfg initramfs initramfscfg interactiveterminal
keyboard keyboardq license locale localecfg localeq luksbootkeyfile
luksopenswaphookcfg machineid mkinitfs mount netinstall networkcfg notesqml oemid
openrcdmcryptcfg packagechooser packagechooserq packages partition plasmalnf
plymouthcfg preservefiles rawfs removeuser services-openrc services-systemd
shellprocess summary summaryq tracking umount unpackfs unpackfsc users usersq
welcome welcomeq zfs zfshostid"
unknown=""
while read -r module; do
    [[ -z "$module" ]] && continue
    if ! grep -qw -- "$module" <<<"$known_modules"; then
        unknown="$unknown $module"
    fi
done < <(sed -n '/^sequence:/,/^branding:/p' "$CA/settings.conf" \
         | sed -n 's/^[[:space:]]*-[[:space:]]*\([a-z][a-z0-9-]*\)$/\1/p')
if [[ -z "$unknown" ]]; then
    ok "every module in the sequence exists in calamares 3.4.2"
else
    bad "sequence names modules calamares does not have:$unknown"
fi
check_grep "unpackfs reads the live squashfs" 'airootfs.sfs' "$CA/modules/unpackfs.conf"
# unpackfs runs `unsquashfs -l` to count files and rsync to copy them; without
# either one the job fails after the disk has already been formatted.
check_grep "unpackfs excludes the live sudoers drop-in" '10-tahoe-live' "$CA/modules/unpackfs.conf"
check_grep "unpackfs excludes the archiso initramfs hook" 'mkinitcpio.conf.d/archiso.conf' "$CA/modules/unpackfs.conf"
check_grep "unpackfs excludes the live getty autologin" 'getty@tty1.service.d/autologin.conf' "$CA/modules/unpackfs.conf"
check_grep "the cleanup job runs after unpackfs" '^[[:space:]]*- shellprocess$' "$CA/settings.conf"
check_grep "the cleanup puts the kernel back in /boot" '/boot/vmlinuz-linux' "$CA/modules/shellprocess.conf"
check_grep "the cleanup locks root" 'passwd -l root' "$CA/modules/shellprocess.conf"
check_grep "the cleanup removes the live user" 'userdel -r tahoe' "$CA/modules/shellprocess.conf"
check_grep "root is not given a password page" '^setRootPassword: false' "$CA/modules/users.conf"
check_grep "launcher goes through the wrapper" '^Exec=tahoe-installer$' \
    iso/airootfs/usr/share/applications/tahoe-install.desktop
check_grep "the live session can reach it"   'tahoe-installer' "$LUA"
# Unprivileged Calamares refuses to install, and a sudo that drops the display
# environment starts a Calamares with no window — both look like a broken
# launcher rather than a broken wrapper, so the wrapper is checked.
check_grep "wrapper elevates"                'sudo -n' iso/airootfs/usr/local/bin/tahoe-installer
check_grep "wrapper keeps the display env"   'preserve-env=' iso/airootfs/usr/local/bin/tahoe-installer
# Calamares 3.4.2's command line is -d, -D, -T, -c/--config, -X. An unknown
# option makes it print a message and exit, which from the desktop looks like
# a launcher that does nothing.
check_not_grep "wrapper passes no option calamares lacks" '^exec .*--branding' \
    iso/airootfs/usr/local/bin/tahoe-installer
# Without this Qt takes the xcb backend because DISPLAY is set, finds no X
# server, and aborts before any window exists.
check_grep "wrapper selects the Wayland Qt backend" 'QT_QPA_PLATFORM=wayland' \
    iso/airootfs/usr/local/bin/tahoe-installer
# mkarchiso does not preserve modes from the checkout, so 0755 in git is 0644
# in the image unless file_permissions says otherwise.
check_grep "profiledef makes the wrapper executable in the image" \
    '/usr/local/bin/tahoe-installer"\]="0:0:755"' iso/profiledef.sh
check_grep "packages.x86_64 lists calamares" '^calamares$' "$PKG"
# The install path's runtime dependencies, found missing in a running live
# image rather than in review.
for tool in rsync squashfs-tools grub efibootmgr dosfstools; do
    check_grep "packages.x86_64 lists $tool" "^$tool$" "$PKG"
done
check_grep "build.sh drops calamares when unbuilt" "SKIPPED_PKG.*calamares" iso/build.sh
# A PKGBUILD's source is a GitHub tag, and CI used to package that tag rather
# than the commit it had just checked out: run 39389036727 shipped v0.1.0,
# 18 commits behind, with the old binfmt rule. The build must snapshot HEAD.
check_grep "build.sh packages the checked-out commit" 'archive --format=tar.gz' iso/build.sh
check_grep "build.sh points PKGBUILDs at that snapshot" 'source=.*tahoe-src.tar.gz' iso/build.sh
# The binfmt rule must name an interpreter that is actually packaged. It named
# /usr/bin/wine, which the image does not install, and systemd-binfmt failed at
# every boot with "Failed to add binary format 'DOSWin'".
check_grep "binfmt runs the dispatcher" '/usr/lib/tahoe/tahoe-dispatch' binfmt.d/wine.conf
check_not_grep "binfmt does not point at an unpackaged wine" '/usr/bin/wine' binfmt.d/wine.conf
check_grep "tahoe-uel installs the dispatcher" 'target/release/tahoe-dispatch' packages/tahoe-uel/PKGBUILD
check_grep "os-release names the distribution" '^NAME="Project Tahoe"' iso/airootfs/etc/os-release
# The live MOTD's claims ("root has no password", "sudo needs no password")
# are false on the installed system, and the cleanup used to copy it verbatim.
check_grep "the installed motd names the distribution" '^Project Tahoe 0.2' \
    iso/airootfs/etc/motd.installed
check_not_grep "the installed motd has no live-only password claim" \
    'no password|without a password' iso/airootfs/etc/motd.installed
check_grep "cleanup installs the post-install motd" \
    'cp /etc/motd.installed /etc/motd' \
    iso/airootfs/etc/calamares/modules/shellprocess.conf
check_not_grep "the motd does not advertise a removed keybind" \
    'reload config' iso/airootfs/etc/motd

# Every module named in the exec/show sequences should exist as a config, or
# Calamares silently falls back to module defaults that were never meant for
# this image. Only unpackfs is written so far; the rest are a known gap and
# this check exists to keep them from being forgotten.
MISSING=()
while read -r mod; do
    [[ -z "$mod" ]] && continue
    [[ -f "$CA/modules/$mod.conf" ]] || MISSING+=("$mod")
done < <(sed -n '/^sequence:/,/^branding:/p' "$CA/settings.conf" \
             | grep -oE '^ +- [a-z0-9-]+$' | awk '{print $2}' | sort -u)
if [[ "${#MISSING[@]}" -eq 0 ]]; then
    ok "every module named in sequence: has a config file"
else
    skip "${#MISSING[@]} module config(s) not written yet (Calamares defaults apply): ${MISSING[*]:0:6} …"
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
