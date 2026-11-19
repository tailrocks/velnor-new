#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Qualification mode `mise-pin`: after the pinned `jdx/mise-action` step, the
# job runs the checked-out ref, and `mise` is the requested version with the
# requested SHA-256. The engine wrote this as a `shell: bash` step; the shell
# is set here instead, because velnor-gen refuses a `shell` key.
#
# MISE_VERSION and MISE_SHA256 come from the step `env`.

set -euo pipefail

: "${MISE_VERSION:?missing MISE_VERSION}"
: "${MISE_SHA256:?missing MISE_SHA256}"

test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
version="$(mise --version)"
case "$version" in
  "$MISE_VERSION linux-x64 ("[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]")") ;;
  *)
    echo "unexpected Mise version: $version" >&2
    exit 1
    ;;
esac
binary="$(command -v mise)"
actual="$(sha256sum "$binary" | awk '{print $1}')"
test "$actual" = "$MISE_SHA256"
