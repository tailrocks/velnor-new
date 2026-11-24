#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

cd "$(dirname "$0")/.."

fixtures=scripts/fixtures
if bash scripts/check-helper-gates.sh --diff "$fixtures/gate-bad.diff" --current 0.1.100; then
  echo "the check accepted a gate that is already released" >&2
  exit 1
fi
if bash scripts/check-helper-gates.sh --diff "$fixtures/gate-correction-unmarked.diff" --current 0.1.100; then
  echo "the check accepted an unmarked correction of a released gate" >&2
  exit 1
fi
if bash scripts/check-helper-gates.sh --diff "$fixtures/gate-correction-wrong-tag.diff" --current 0.1.100; then
  echo "the check accepted a correction marker that names the wrong tag" >&2
  exit 1
fi
if bash scripts/check-helper-gates.sh --diff "$fixtures/gate-correction-previous.diff" --current 0.1.100; then
  echo "the check accepted a correction whose commit already shipped in the previous release" >&2
  exit 1
fi
if bash scripts/check-helper-gates.sh --diff "$fixtures/gate-prose.diff" --current 0.1.100; then
  echo "the check accepted the prose gate marker" >&2
  exit 1
fi
bash scripts/check-helper-gates.sh --diff "$fixtures/gate-good.diff" --current 0.1.100
bash scripts/check-helper-gates.sh --diff "$fixtures/gate-correction-good.diff" --current 0.1.100
out=$(bash scripts/check-helper-gates.sh --diff "$fixtures/gate-correction-untagged.diff" --current 0.1.111)
case "$out" in
  *"first in tools-v0.1.111 via 66940bed2e"*) ;;
  *) echo "the check did not name the release tag tools-v0.1.111: $out" >&2; exit 1 ;;
esac
out=$(bash scripts/check-helper-gates.sh --diff "$fixtures/gate-correction-legacy-rust-ci.diff" --current 0.1.100)
case "$out" in
  *"first in rust-ci-v0.1.82 via 6d583dd497"*) ;;
  *) echo "the check did not name the release tag rust-ci-v0.1.82: $out" >&2; exit 1 ;;
esac
for f in gate-unreleased-untagged gate-unreleased-released-marker gate-prose-released gate-released-unmarked gate-marker-unchanged-wrong-commit gate-marker-unchanged-wrong-version gate-path-with-delimiter gate-char-literal; do
  if bash scripts/check-helper-gates.sh --diff "$fixtures/$f.diff" --current 0.1.110; then
    echo "the check accepted $f.diff" >&2
    exit 1
  fi
done
bash scripts/check-helper-gates.sh --diff "$fixtures/gate-unreleased-none.diff" --current 0.1.110
bash scripts/check-helper-gates.sh --diff "$fixtures/gate-released-marked.diff" --current 0.1.110
bash scripts/check-helper-gates.sh --diff "$fixtures/gate-marker-unchanged-good.diff" --current 0.1.110
bash scripts/check-helper-gates.sh --diff "$fixtures/gate-prose-in-code.diff" --current 0.1.110
bash scripts/check-helper-gates.sh --diff "$fixtures/gate-bump-marked.diff" --current 0.1.111
if bash scripts/check-helper-gates.sh --diff "$fixtures/gate-bump-unmarked.diff" --current 0.1.111; then
  echo "the check accepted an unmarked gate in the release bump" >&2
  exit 1
fi
rel=(--base-version 0.1.110 --tag-version 0.1.110 --current 0.1.111)
if bash scripts/check-helper-gates.sh --diff /dev/null --tree "$fixtures/gate-tree-unmarked.diff" "${rel[@]}"; then
  echo "the check accepted an unmarked gate already in the tree at the release bump" >&2
  exit 1
fi
bash scripts/check-helper-gates.sh --diff /dev/null --tree "$fixtures/gate-tree-marked.diff" "${rel[@]}"
bash scripts/check-helper-gates.sh --diff /dev/null --tree "$fixtures/gate-tree-future.diff" "${rel[@]}"
bash scripts/check-helper-gates.sh --diff "$fixtures/gate-marker-in-code.diff" --current 0.1.110
if bash scripts/check-helper-gates.sh --diff "$fixtures/gate-marker-malformed.diff" --current 0.1.110; then
  echo "the check accepted a malformed marker comment" >&2
  exit 1
fi
if bash scripts/check-helper-gates.sh --diff "$fixtures/gate-released-unmarked.diff" --current 0.1.109 --base-version 0.1.110 --tag-version 0.1.110; then
  echo "the check let a lowered head version hide a released gate" >&2
  exit 1
fi
