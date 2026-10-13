#!/bin/sh
# SPDX-License-Identifier: 0BSD
# Smoke test: regenerate fixtures, probe each with the UEL CLI, show host facts.
# Usage: sh tests/uel-smoke.sh   (from the repo root)
set -eu
python3 fixtures/generate.py fixtures/
cargo build -q -p tahoe-uel
./target/debug/tahoe-uel host
for f in fixtures/*; do
  [ -f "$f" ] || continue
  ./target/debug/tahoe-uel probe "$f" || true
  echo "---"
done
