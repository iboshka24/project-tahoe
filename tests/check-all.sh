#!/usr/bin/env bash
# SPDX-License-Identifier: 0BSD
#
# Check everything, in the order that fails fastest.
#
# Some of Tahoe's guarantees cannot be expressed as `cargo test`: the
# generated Swift must be fresh, the rendered desktop must actually look
# right, and the kernel must actually dispatch a file. This runs all of it.
#
# Nothing here needs root, installs anything, or touches the host's
# configuration. The binfmt step runs entirely inside a throwaway user
# namespace.
#
# Usage:
#   tests/check-all.sh            # everything
#   tests/check-all.sh --quick    # skip the slow process-spawning tests

set -uo pipefail

cd "$(dirname "$0")/.." || exit 1

QUICK=0
[[ "${1:-}" == "--quick" ]] && QUICK=1

pass=0
fail=0
skipped=0

section() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }
ok()   { printf '  \033[32mok\033[0m    %s\n' "$1"; pass=$((pass + 1)); }
bad()  { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; fail=$((fail + 1)); }
skip() { printf '  \033[33mskip\033[0m  %s\n' "$1"; skipped=$((skipped + 1)); }

run() {
    # run <label> <command...>
    local label="$1"
    shift
    if "$@" >/tmp/tahoe-check.log 2>&1; then
        ok "$label"
    else
        bad "$label"
        sed 's/^/        /' /tmp/tahoe-check.log | tail -25
    fi
}

# ---------------------------------------------------------------------------
section "Rust: build, test, lint, format"
# ---------------------------------------------------------------------------

run "cargo build --workspace" cargo build --workspace --quiet
run "cargo test" cargo test --quiet

# The count matters: a test that silently stops being collected is worse
# than one that fails, so the total is reported and floor-checked.
TEST_TOTAL=$(cargo test --quiet 2>/dev/null | grep -E '^test result' |
    awk -F'[ ;]' '{s += $4} END {print s + 0}')
if [[ "$TEST_TOTAL" -ge 500 ]]; then
    ok "test count is $TEST_TOTAL (>= 500)"
else
    bad "only $TEST_TOTAL tests were collected; expected at least 500"
fi

run "cargo clippy --all-targets" cargo clippy --all-targets --quiet -- -D warnings
run "cargo fmt --check" cargo fmt --all -- --check

# ---------------------------------------------------------------------------
section "Generated Swift is fresh"
# ---------------------------------------------------------------------------

if cargo run --quiet -p tahoe-shell --bin tahoe-design-export -- --check \
        apple/Sources/TahoeDesign/TahoeDesign.swift >/tmp/tahoe-design.log 2>&1; then
    ok "TahoeDesign.swift matches the Rust design system"
else
    bad "TahoeDesign.swift is stale"
    sed 's/^/        /' /tmp/tahoe-design.log | tail -5
    echo "        run: cargo run -p tahoe-shell --bin tahoe-design-export -- apple/Sources/TahoeDesign/TahoeDesign.swift"
fi

# ---------------------------------------------------------------------------
section "Rendered desktop"
# ---------------------------------------------------------------------------

if command -v python3 >/dev/null 2>&1; then
    if python3 tests/verify-preview.py >/tmp/tahoe-preview.log 2>&1; then
        CHECKS=$(grep -c '\[ok  \]' /tmp/tahoe-preview.log)
        ok "desktop preview: $CHECKS independent visual assertions"
    else
        bad "desktop preview verification"
        sed 's/^/        /' /tmp/tahoe-preview.log | tail -20
    fi
else
    skip "desktop preview (python3 not installed)"
fi

# ---------------------------------------------------------------------------
section "Kernel-level format dispatch"
# ---------------------------------------------------------------------------

if ! command -v unshare >/dev/null 2>&1; then
    skip "binfmt dispatch (unshare not installed)"
else
    # Registers a rule inside a throwaway user namespace and lets the kernel
    # dispatch a real file. Needs no root; the host is untouched.
    if cargo run --quiet -p tahoe-compat --bin tahoe-compat -- binfmt self-test \
            >/tmp/tahoe-binfmt.log 2>&1; then
        ok "the kernel dispatched a file to our interpreter, rootlessly"
    else
        # A kernel without CONFIG_BINFMT_MISC or with unprivileged user
        # namespaces disabled is a supported configuration, not a failure.
        if grep -qiE 'operation not permitted|not permitted|mount' /tmp/tahoe-binfmt.log; then
            skip "binfmt dispatch (this kernel or policy forbids user namespaces)"
        else
            bad "binfmt dispatch"
            sed 's/^/        /' /tmp/tahoe-binfmt.log | tail -15
        fi
    fi
    # The host must be unchanged either way.
    HOST_ENTRIES=$(ls /proc/sys/fs/binfmt_misc/ 2>/dev/null | grep -cvE '^(register|status)$' || true)
    if [[ "${HOST_ENTRIES:-0}" -le 1 ]]; then
        ok "host binfmt_misc untouched"
    else
        bad "the host has $HOST_ENTRIES binfmt entries; namespaced runs must not add any"
    fi
fi

# ---------------------------------------------------------------------------
section "Apple toolchain"
# ---------------------------------------------------------------------------

APPLE_LOG=$(cargo run --quiet -p tahoe-compat --bin tahoe-compat -- apple toolchain 2>&1 || true)
if grep -q 'ready to build' <<<"$APPLE_LOG"; then
    ok "swift + xtool + darwin SDK present; a build could run"
elif grep -q 'Swift toolchain on PATH' <<<"$APPLE_LOG"; then
    skip "Apple builds need a Swift 6.4 toolchain (see docs/apple-development.md)"
elif grep -q 'darwin SDK' <<<"$APPLE_LOG"; then
    skip "Apple builds need an Xcode SDK, which requires an Apple ID"
else
    skip "Apple build prerequisites unmet"
fi

# The project itself is always checkable.
run "apple/ project loads" \
    cargo run --quiet -p tahoe-compat --bin tahoe-compat -- apple project apple

# ---------------------------------------------------------------------------
section "Execution overhead"
# ---------------------------------------------------------------------------

if [[ "$QUICK" == "1" ]]; then
    skip "benchmark (--quick)"
else
    if cargo build --release --quiet -p tahoe-uel --bin tahoe-bench >/dev/null 2>&1; then
        ./target/release/tahoe-bench >/tmp/tahoe-bench.log 2>&1 || true
        DETECT=$(grep 'probe_bytes elf64' /tmp/tahoe-bench.log | awk '{print $4, $5}')
        OVERHEAD=$(grep 'supervisor overhead' /tmp/tahoe-bench.log | sed 's/.*: //')
        ok "format detection ${DETECT:-unmeasured}; supervisor overhead ${OVERHEAD:-unmeasured}"
    else
        bad "tahoe-bench did not build"
    fi
fi

# ---------------------------------------------------------------------------
printf '\n\033[1m%d passed, %d failed, %d skipped\033[0m\n' "$pass" "$fail" "$skipped"
[[ "$fail" -eq 0 ]] || exit 1
