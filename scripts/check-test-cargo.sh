#!/bin/sh

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -eu

allowlist=${TEST_CARGO_ALLOWLIST:-scripts/check-test-cargo.allowlist}
pattern='env!\("CARGO"\)|var(_os)?\("CARGO"\)|Command::new\("cargo"\)|command\("cargo"\)|run\([^,]+, "cargo"|exec cargo|program\("cargo"'

allowed=$(sed -e 's/[[:space:]]*#.*//' -e '/^[[:space:]]*$/d' "$allowlist")
hits=$(git grep -nE "$pattern" -- ':(glob)crates/*/tests/**/*.rs' ':!crates/velnor-test-support' || true)

bad=
if [ -n "$hits" ]; then
  bad=$(printf '%s\n' "$hits" | while IFS= read -r line; do
    file=${line%%:*}
    printf '%s\n' "$allowed" | grep -qxF -- "$file" || printf '%s\n' "$line"
  done)
fi

if [ -n "$bad" ]; then
  printf '%s\n' "$bad"
  echo "check-test-cargo: use velnor_test_support::real_cargo() (or mirror_cargo() for wrapper tests)" >&2
  exit 1
fi
