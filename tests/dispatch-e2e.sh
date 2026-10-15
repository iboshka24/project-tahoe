#!/usr/bin/env bash
# SPDX-License-Identifier: 0BSD
#
# Prove the execution path end to end.
#
#   kernel ──binfmt_misc──▶ tahoe-dispatch ──execv──▶ wine ──▶ program
#
# The second hop is the point: `tahoe-dispatch` calls `execv`, replacing its
# own process image. It does not fork, does not stay resident, and does not
# appear in the process tree. Two processes exist for the whole launch — the
# same count a `#!` script produces.
#
# If a real `wine` is on PATH, this compiles a genuine Windows executable
# with clang + lld-link (both are on Arch, so no mingw is needed) and runs it
# for real. Otherwise it uses a stand-in that records its argv, which still
# proves the hand-off.
#
# No root, and nothing on the host changes: the binfmt registration happens
# inside a throwaway user namespace.
#
# Usage: tests/dispatch-e2e.sh

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

pass=0
fail=0
ok()  { printf '  \033[32mok\033[0m    %s\n' "$1"; pass=$((pass + 1)); }
bad() { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; fail=$((fail + 1)); }

command -v unshare >/dev/null 2>&1 || { echo "unshare is not installed; skipping"; exit 0; }

echo "== building the dispatcher =="
cargo build --quiet -p tahoe-uel --bin tahoe-dispatch || { echo "build failed"; exit 1; }
DISPATCH="$PWD/target/debug/tahoe-dispatch"
[[ -x "$DISPATCH" ]] || { echo "no dispatcher at $DISPATCH"; exit 1; }

# Under /tmp/opencode when it exists, because a sandboxed layer (Wine from a
# Flatpak, which is how it reaches a machine with no root) cannot see an
# arbitrary mktemp directory unless it is granted access to it.
# Prefer /tmp/opencode: a sandboxed Wine (installed as a Flatpak, which is
# how it reaches a machine with no root) is usually granted that path, and
# `flatpak run` does not accept options after the app id, so the test cannot
# add the grant itself afterwards.
E2E_PARENT="${TAHOE_E2E_WORK:-}"
if [[ -z "$E2E_PARENT" ]]; then
    if [[ -d /tmp/opencode ]]; then E2E_PARENT=/tmp/opencode; else E2E_PARENT=/tmp; fi
fi
WORK=$(mktemp -d "$E2E_PARENT/tahoe-e2e-XXXXXX")
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin"

# --- the layer -----------------------------------------------------------------

REAL_WINE=""
if command -v wine >/dev/null 2>&1; then
    REAL_WINE=$(command -v wine)
    echo "   real Wine: $REAL_WINE"
else
    echo "   no Wine on PATH; using a stand-in"
fi

{
    echo '#!/bin/sh'
    echo "printf 'wine-argv:'"
    echo 'for a in "$@"; do printf " [%s]" "$a"; done'
    echo 'printf "\n"'
    if [[ -n "$REAL_WINE" ]]; then
        echo "exec \"$REAL_WINE\" \"\$@\""
    else
        echo 'exit 0'
    fi
} > "$WORK/bin/wine"
chmod +x "$WORK/bin/wine"

# --- the program ---------------------------------------------------------------

WANT_OUTPUT=0
if command -v clang >/dev/null 2>&1 && command -v lld-link >/dev/null 2>&1; then
    mkdir -p "$WORK/pe"
    cat > "$WORK/pe/app.c" <<'CEOF'
typedef unsigned long DWORD;
typedef void *HANDLE;
__declspec(dllimport) HANDLE __stdcall GetStdHandle(DWORD);
__declspec(dllimport) int __stdcall WriteFile(HANDLE, const void *, DWORD, DWORD *, void *);
__declspec(dllimport) void __stdcall ExitProcess(unsigned int);
void mainCRTStartup(void) {
    DWORD n = 0;
    WriteFile(GetStdHandle((DWORD)-11), "tahoe-pe-ok\n", 12, &n, 0);
    ExitProcess(0);
}
CEOF
    printf 'LIBRARY kernel32.dll\nEXPORTS\n  GetStdHandle\n  WriteFile\n  ExitProcess\n' > "$WORK/pe/k.def"
    if ( cd "$WORK/pe" \
            && clang --target=x86_64-pc-windows-msvc -ffreestanding -fno-stack-protector -c app.c -o app.obj \
            && lld-link /lib /def:k.def /out:k.lib /machine:x64 \
            && lld-link /entry:mainCRTStartup /subsystem:console /nodefaultlib /out:installer.exe app.obj k.lib ) >/dev/null 2>&1
    then
        cp "$WORK/pe/installer.exe" "$WORK/installer.exe"
        echo "   compiled a real Windows PE with clang + lld-link"
        [[ -n "$REAL_WINE" ]] && WANT_OUTPUT=1
    fi
fi

if [[ ! -f "$WORK/installer.exe" ]]; then
    # Fallback: a parseable PE header written by hand. Proves the parser but
    # not execution.
    python3 - "$WORK/installer.exe" <<'PY'
import struct, sys
b = bytearray(64)
b[0:2] = b"MZ"
struct.pack_into("<I", b, 0x3C, 0x40)
b += b"PE\0\0" + struct.pack("<H", 0x8664) + bytes(18)
b += struct.pack("<H", 0x20B)
while len(b) < 0x40 + 24 + 68:
    b.append(0)
b += struct.pack("<H", 3) + bytes(16)
open(sys.argv[1], "wb").write(bytes(b))
PY
    echo "   built a synthetic PE header (clang or lld-link missing)"
fi
chmod +x "$WORK/installer.exe"

# Things the rule must NOT claim.
printf '#!/bin/sh\necho "a script"\n' > "$WORK/script.sh"
chmod +x "$WORK/script.sh"
python3 - "$WORK/Thing.class" <<'PY'
import sys
open(sys.argv[1], "wb").write(bytes([0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, 0x00, 0x34]) + bytes(64))
PY
chmod +x "$WORK/Thing.class"

# --- run inside a namespace ----------------------------------------------------

echo
echo "== registering the rules in a throwaway namespace =="

RUN=$(cat <<'SCRIPT'
mount -t binfmt_misc binfmt_misc "$MNT" 2>/dev/null || true
# P preserves argv[0]. No O, C or F: none is needed, and C would change
# credential handling.
printf ':%s:M:0:\x4d\x5a::%s:P\n' tahoe-e2e-pe "$DISPATCH" > "$MNT/register"
# The fat-Mach-O magic is also the Java class magic, so the dispatcher has
# to undo a match the kernel cannot avoid making.
printf ':%s:M:0:\xca\xfe\xba\xbe::%s:P\n' tahoe-e2e-fat "$DISPATCH" > "$MNT/register"
echo '--- entry as the kernel sees it ---'
cat "$MNT/tahoe-e2e-pe"
echo '--- ./installer.exe with arguments ---'
"$WORK/installer.exe" --silent /install
echo '--- ./installer.exe as a bare path, the way a file manager would ---'
cd "$WORK" && ./installer.exe
echo '--- a shell script must run natively ---'
"$WORK/script.sh"
echo '--- a Java class must be refused despite matching the FAT magic ---'
if "$WORK/Thing.class" >/dev/null 2>&1; then echo CLASS-ACCEPTED; else echo class-refused; fi
"$WORK/Thing.class" 2>&1 | head -3 || true
SCRIPT
)

OUT=$(MNT=/proc/sys/fs/binfmt_misc DISPATCH="$DISPATCH" WORK="$WORK" \
    PATH="$WORK/bin:$PATH" \
    unshare -Ur -m -- sh -c "$RUN" 2>&1) || true

# Wine is chatty; keep the noise out of the assertions.
CLEAN=$(grep -vE '^(fixme:|err:|warn:)|wineusb|winemenubuilder|wineboot' <<<"$OUT")
echo "$CLEAN"
echo

# --- assertions ----------------------------------------------------------------

grep -q 'interpreter .*tahoe-dispatch' <<<"$OUT" \
    && ok "the kernel registered our dispatcher as the interpreter" \
    || bad "the entry does not name tahoe-dispatch"

grep -qE 'wine-argv: \[.*installer\.exe\] \[--silent\] \[/install\]' <<<"$OUT" \
    && ok "the layer received the binary and the arguments, in order" \
    || bad "the layer was not invoked with the expected argv"

BARE=$(grep -c 'wine-argv:' <<<"$OUT")
[[ "$BARE" -ge 2 ]] \
    && ok "a bare ./installer.exe dispatches the same way" \
    || bad "only $BARE invocations; the bare path did not dispatch"

if [[ "$WANT_OUTPUT" == "1" ]]; then
    grep -q 'tahoe-pe-ok' <<<"$OUT" \
        && ok "the compiled Windows executable actually ran under Wine" \
        || bad "the .exe did not produce its output"
else
    ok "execution itself not attempted (no real Wine on PATH)"
fi

grep -q 'a script' <<<"$OUT" && ! grep -qE 'wine-argv: \[.*script\.sh' <<<"$OUT" \
    && ok "a shell script ran natively and never reached the dispatcher" \
    || bad "the script either did not run or was routed to the layer"

grep -q 'class-refused' <<<"$OUT" && ! grep -q 'CLASS-ACCEPTED' <<<"$OUT" \
    && ok "a Java class was refused despite matching the magic" \
    || bad "a Java class was accepted; the magic-to-layer mapping is wrong"

grep -q 'java-class' <<<"$OUT" \
    && ok "the refusal names the real format rather than the matched one" \
    || bad "the refusal does not say what the file actually is"

HOST_ENTRIES=$(ls /proc/sys/fs/binfmt_misc/ 2>/dev/null | grep -cvE '^(register|status)$' || true)
[[ "${HOST_ENTRIES:-0}" -le 1 ]] \
    && ok "the host's dispatcher was not modified" \
    || bad "the host has $HOST_ENTRIES entries; the namespace leaked"

echo
printf '\033[1m%d passed, %d failed\033[0m\n' "$pass" "$fail"
[[ "$fail" -eq 0 ]] || exit 1
