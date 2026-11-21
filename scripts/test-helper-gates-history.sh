#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
checker="$root/scripts/check-helper-gates.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

git init -q
git config user.name ci
git config user.email ci@invalid
mkdir -p crates/velnor-ci crates/velnor-gen/src
printf '[package]\nname = "velnor-ci"\nversion = "0.1.110"\n' >crates/velnor-ci/Cargo.toml
{
  echo 'const RAW: &str = "'
  for i in 1 2 3 4 5; do echo "line $i"; done
  echo '";'
} >crates/velnor-gen/src/policy.rs
git add -A
git commit -qm base
git tag tools-v0.1.109
echo marker >notes.txt
git add -A
git commit -qm "the commit a correction names"
marker=$(git rev-parse HEAD)
git tag tools-v0.1.110

git checkout -q -b in-literal
{
  echo 'const RAW: &str = "'
  for i in 1 2 3 4 5; do echo "line $i"; done
  echo "    x; // helper-gate: first in tools-v0.1.110 via $marker \";"
  echo '    gate(RustCi, "budget", (0, 1, 110)),'
} >crates/velnor-gen/src/policy.rs
git commit -qam "marker inside the literal"
if bash "$checker" "$marker"; then
  echo "the check accepted a marker inside a string literal opened above the diff context" >&2
  exit 1
fi

git checkout -q -b in-comment "$marker"
{
  echo 'const RAW: &str = "'
  for i in 1 2 3 4 5; do echo "line $i"; done
  echo '";'
  echo "// helper-gate: first in tools-v0.1.110 via $marker"
  echo '    gate(RustCi, "budget", (0, 1, 110)),'
} >crates/velnor-gen/src/policy.rs
git commit -qam "marker in a comment"
bash "$checker" "$marker"

mkdir -p docs
echo 'The key needs velnor-ci 0.1.110 or later (first released in tools-v0.1.110).' >docs/rust-ci.md
git add docs/rust-ci.md
git commit -qm "prose release marker in a doc"
if bash "$checker" "$marker"; then
  echo "the check accepted a prose release marker in a documentation page" >&2
  exit 1
fi
echo 'The key needs velnor-ci 0.1.110 or later.' >docs/rust-ci.md
git commit -qam "doc states only the version"
bash "$checker" "$marker"
