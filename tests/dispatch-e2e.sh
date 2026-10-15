#!/usr/bin/env bash
# SPDX-License-Identifier: 0BSD
#
# Prove the "native" execution path end to end, with a stand-in for Wine.
#
# What this demonstrates:
#
#   kernel  ──binfmt_misc──▶  tahoe-dispatch  ──execv──▶  wine  ──▶  program
#            (1 hop)                       (1 hop, replacing itself)
#
# The point is the second hop: `tahoe-dispatch` calls `execv`, which replaces
# its own process image. It does not fork, does not stay resident, and does
# not appear in the process tree. Two processes exist for the whole launch —
# the same count a `#!` script produces.
#
# Wine is substituted by a script that prints how it was invoked, so this
# runs on a machine with no Wine, no root, and no changes to the host: the
# binfmt registration happens inside a throwaway user namespace.
#
# Usage: tests/dispatch-e2e.sh

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

pass=0
fail=0
ok()   { printf '  \033[32mok\033[0m    %s\n' "$1"; pass=$((pass + 1)); }
bad()  { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; fail=$((fail + 1)); }

if ! command -v unshare >/dev/null 2>&1; then
    echo "unshare is not installed; skipping"
    exit 0
fi

echo "== building the dispatcher =="
cargo build --quiet -p tahoe-uel --bin tahoe-dispatch || { echo "build failed"; exit 1; }
DISPATCH="$PWD/target/debug/tahoe-dispatch"
[[ -x "$DISPATCH" ]] || { echo "no dispatcher at $DISPATCH"; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin"

# A stand-in for Wine. It records its own argv so the test can show exactly
# how the kernel and the dispatcher handed the program over.
cat > "$WORK/bin/wine" <<'WINE'
#!/bin/sh
printf 'wine-argv:'
for a in "$@"; do printf ' [%s]' "$a"; done
printf '\n'
exit 0
WINE
chmod +x "$WORK/bin/wine"

# A minimal but genuinely parseable PE image: `MZ`, `e_lfanew` at 0x3C, a
# `PE\0\0` signature, COFF machine x86-64, PE32+ optional header, and a
# console subsystem. The dispatcher re-parses this rather than trusting the
# `MZ` magic, so it has to be real.
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
chmod +x "$WORK/installer.exe"

# Something the rule must NOT claim.
printf '#!/bin/sh\necho "a script"\n' > "$WORK/script.sh"
chmod +x "$WORK/script.sh"
python3 - "$WORK/Thing.class" <<'PY'
import sys
b = bytearray([0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, 0x00, 0x34]) + bytearray(64)
open(sys.argv[1], "wb").write(bytes(b))
PY
chmod +x "$WORK/Thing.class"

echo
echo "== registering the rule inside a throwaway namespace and running the file =="
echo "   (Wine is a stand-in; nothing is installed and the host is untouched)"

# One script for the whole flow: the mount, the registration, and the exec
# must happen in the same namespace.
RUN=$(cat <<'SCRIPT'
set -e
mount -t binfmt_misc binfmt_misc "$MNT" 2>/dev/null || true
# Flags: P preserves argv[0], so the dispatcher receives the binary and a
# copy of what the user typed. No O, C or F: none of them is needed and C
# would change credential handling.
printf ':%s:M:0:\\x4d\\x5a::%s:P\n' tahoe-e2e-pe "$DISPATCH" > "$MNT/register"
# The fat-Mach-O magic is also the Java class magic. A real rule would be
# registered for it, so the refusal has to happen in the dispatcher.
printf ':%s:M:0:\\xca\\xfe\\xba\\xbe::%s:P\n' tahoe-e2e-fat "$DISPATCH" > "$MNT/register"
echo "--- entry as the kernel sees it ---"
cat "$MNT/tahoe-e2e-pe"
echo "--- running ./installer.exe ---"
"$WORK/installer.exe" --silent /install
echo "--- running it as a bare path, the way a file manager would ---"
cd "$WORK" && ./installer.exe
echo "--- a shell script must run natively, not through the dispatcher ---"
"$WORK/script.sh"
echo "--- the dispatcher must refuse a Java class that matched the FAT rule ---"
if "$WORK/Thing.class" >/dev/null 2>&1; then
    echo "CLASS-ACCEPTED"
else
    echo "class-refused"
fi
"$WORK/Thing.class" 2>&1 | head -3 || true
SCRIPT
)

OUT=$(MNT=/proc/sys/fs/binfmt_misc DISPATCH="$DISPATCH" WORK="$WORK" \
    PATH="$WORK/bin:$PATH" \
    unshare -Ur -m -- sh -c "$RUN" 2>&1) || true
echo "$OUT"
echo

# --- assertions -------------------------------------------------------------

if grep -q 'interpreter .*tahoe-dispatch' <<<"$OUT"; then
    ok "the kernel registered our dispatcher as the interpreter"
else
    bad "the entry does not name tahoe-dispatch"
fi

if grep -qE 'wine-argv: \[.*installer\.exe\] \[--silent\] \[/install\]' <<<"$OUT"; then
    ok "wine received the binary and the arguments, in order"
else
    bad "wine was not invoked with the expected argv"
fi

# The bare `./installer.exe` invocation is the whole point: no launcher, no
# wrapper, just the file.
BARE=$(grep -c 'wine-argv:' <<<"$OUT")
if [[ "$BARE" -ge 2 ]]; then
    ok "a bare ./installer.exe works the same as an explicit invocation"
else
    bad "only $BARE wine invocations; the bare path did not dispatch"
fi

# A `#!` script must still run natively. The MZ rule does not match it, so
# nothing changes for the common case — that is the property worth pinning.
if grep -q 'a script' <<<"$OUT" && ! grep -qE 'wine-argv: \[.*script\.sh' <<<"$OUT"; then
    ok "a shell script ran natively and never reached the dispatcher"
else
    bad "the script either did not run or was routed to wine"
fi

# The Java class matched `ca fe ba be`, so the kernel handed it to the
# dispatcher — and the dispatcher has to undo that, because the magic cannot
# tell a universal binary from a class file.
if grep -q 'class-refused' <<<"$OUT" && ! grep -q 'CLASS-ACCEPTED' <<<"$OUT"; then
    ok "a Java class was refused by the dispatcher despite matching the magic"
else
    bad "a Java class was accepted; the magic-to-layer mapping is wrong"
fi

if grep -q 'java-class' <<<"$OUT"; then
    ok "the refusal names the real format rather than the matched one"
else
    bad "the refusal does not say what the file actually is"
fi

# The host must be unchanged: the registration lives in the namespace.
HOST_ENTRIES=$(ls /proc/sys/fs/binfmt_misc/ 2>/dev/null | grep -cvE '^(register|status)$' || true)
if [[ "${HOST_ENTRIES:-0}" -le 1 ]]; then
    ok "the host's dispatcher was not modified"
else
    bad "the host has $HOST_ENTRIES entries; the namespace leaked"
fi

echo
if [[ "$fail" -eq 0 ]]; then
    printf '\033[1m%d passed, %d failed\033[0m\n' "$pass" "$fail"
else
    printf '\033[1m%d passed, %d failed\033[0m\n' "$pass" "$fail"
    exit 1
fi
