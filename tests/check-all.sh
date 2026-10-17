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
section "Shell renders"
# ---------------------------------------------------------------------------
#
# The shell's renderer, drawing the desktop from the design system. Checked
# by decoding the PNG and asking whether it has text in it: a picture with
# no words is the realistic failure, and a build succeeds either way.

if cargo run --quiet -p tahoe-shell --bin tahoe-preview -- --scene desktop \
        --out /tmp/tahoe-shell-check.png >/tmp/tahoe-shell-render.log 2>&1; then
    if python3 tests/verify-shell-render.py /tmp/tahoe-shell-check.png \
            >/tmp/tahoe-shell-verify.log 2>&1; then
        ok "the shell renders a desktop with legible text"
    else
        bad "the rendered desktop failed verification"
        sed 's/^/        /' /tmp/tahoe-shell-verify.log | tail -10
    fi
else
    bad "tahoe-preview failed"
    tail -5 /tmp/tahoe-shell-render.log | sed 's/^/        /'
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
section "Native dispatch path"
# ---------------------------------------------------------------------------
#
# End to end with a stand-in for Wine: the kernel dispatches a real PE to
# tahoe-dispatch, which execv's the layer. Proves the two-hop chain and that
# the dispatcher undoes an over-broad magic match.

if timeout 600 tests/dispatch-e2e.sh >/tmp/tahoe-dispatch-e2e.log 2>&1; then
    D_LAST=$(grep -oE '[0-9]+ passed, [0-9]+ failed' /tmp/tahoe-dispatch-e2e.log | tail -1)
    ok "kernel → dispatcher → layer, end to end (${D_LAST:-?})"
else
    bad "the dispatch path failed"
    tail -20 /tmp/tahoe-dispatch-e2e.log | sed 's/^/        /'
fi

# ---------------------------------------------------------------------------
section "COSMIC theme"
# ---------------------------------------------------------------------------

if cargo run --quiet -p tahoe-shell --bin tahoe-cosmic-theme -- --list \
        >/tmp/tahoe-cosmic.log 2>&1; then
    C_FILES=$(grep -c '/usr/share/cosmic/' /tmp/tahoe-cosmic.log)
    ok "COSMIC theme tree: $C_FILES files generated"
else
    bad "the COSMIC theme generator failed"
    tail -5 /tmp/tahoe-cosmic.log | sed 's/^/        /'
fi

# ---------------------------------------------------------------------------
section "Swift: compile, parse, test"
# ---------------------------------------------------------------------------
#
# The Swift toolchain is not a system package on Arch. `tests/swift-env.sh`
# explains how to install one without root; here we just look for one.

SWIFT_ROOT="${TAHOE_SWIFT_ROOT:-$HOME/.local/share/swift-toolchain}"
if command -v swift >/dev/null 2>&1; then
    SWIFT_BIN="$(command -v swift)"
elif [[ -x "$SWIFT_ROOT/usr/bin/swift" ]]; then
    SWIFT_BIN="$SWIFT_ROOT/usr/bin/swift"
    export PATH="$SWIFT_ROOT/usr/bin:$PATH"
else
    SWIFT_BIN=""
fi

if [[ -z "$SWIFT_BIN" ]]; then
    skip "Swift: no toolchain (see tests/swift-env.sh to install one without root)"
else
    ok "Swift toolchain: $("$SWIFT_BIN" --version 2>&1 | head -1)"

    # 1. The generated tokens must actually compile. This is the check the
    #    whole code-generation arrangement depends on.
    if (cd apple && swift build --target TahoeDesign) >/tmp/tahoe-swift-build.log 2>&1; then
        ok "generated TahoeDesign.swift compiles"
    else
        bad "generated TahoeDesign.swift does not compile"
        sed 's/^/        /' /tmp/tahoe-swift-build.log | tail -20
    fi

    # 2. Every Swift file must at least parse. `-parse` needs no SwiftUI and
    #    no SDK, so it covers the view files that cannot be compiled here.
    parsed=0
    syntax_fail=0
    while IFS= read -r f; do
        parsed=$((parsed + 1))
        if ! swiftc -parse "$f" >/tmp/tahoe-swift-parse.log 2>&1; then
            syntax_fail=$((syntax_fail + 1))
            echo "        syntax error in $f"
            grep -E 'error:' /tmp/tahoe-swift-parse.log | head -3 | sed 's/^/          /'
        fi
    done < <(find apple/Sources apple/Tests -name '*.swift' | sort)
    if [[ "$syntax_fail" -eq 0 ]]; then
        ok "all $parsed Swift files parse"
    else
        bad "$syntax_fail of $parsed Swift files have syntax errors"
    fi

    # 3. The token and model tests must pass.
    if (cd apple && swift test) >/tmp/tahoe-swift-test.log 2>&1; then
        # Each bundle reports its count three times (its own suite, the
        # xctest wrapper, and "All tests"), so the distinct values have to be
        # summed rather than the lines. Averaging them was a bug in the first
        # version of this check and reported 10.5 tests.
        SWIFT_TESTS=$(grep -oE 'Executed [0-9]+ tests' /tmp/tahoe-swift-test.log |
            grep -oE '^Executed [0-9]+' | grep -oE '[0-9]+' | sort -un |
            awk '{s += $1} END {print s + 0}')
        if [[ "${SWIFT_TESTS:-0}" -ge 15 ]]; then
            ok "swift test: ${SWIFT_TESTS} tests passed"
        else
            bad "only ${SWIFT_TESTS:-0} Swift tests ran; expected at least 15"
        fi
    else
        bad "swift test failed"
        grep -E 'error:|failed' /tmp/tahoe-swift-test.log | head -10 | sed 's/^/        /'
    fi
fi

# ---------------------------------------------------------------------------
section "Apple toolchain"
# ---------------------------------------------------------------------------

APPLE_LOG=$(PATH="$PATH" cargo run --quiet -p tahoe-compat --bin tahoe-compat -- apple toolchain 2>&1 || true)
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
