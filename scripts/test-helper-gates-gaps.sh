#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
checker="$root/scripts/check-helper-gates.sh"
fixtures="$root/scripts/fixtures"

for f in gate-const-unmarked gate-const-pub-in gate-const-pub-space gate-string-marker; do
  if bash "$checker" --diff "$fixtures/$f.diff" --current 0.1.110; then
    echo "the check accepted $f.diff" >&2
    exit 1
  fi
done
for f in gate-const-marked gate-const-fn gate-const-block gate-const-string gate-const-string-tuple gate-const-string-escape gate-const-rgb gate-macro-tuple gate-macro-space gate-pattern-tuple gate-pattern-guard; do
  bash "$checker" --diff "$fixtures/$f.diff" --current 0.1.110
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
git init -q
git config user.name ci
git config user.email ci@invalid
mkdir -p crates/velnor-ci generator "generator/old dir"
printf '[package]\nname = "velnor-ci"\nversion = "0.1.110"\n' >crates/velnor-ci/Cargo.toml
echo 'gate(RustCi, "budget", (0, 1, 110)),' >generator/gates.rs
echo "gate(RustCi, \"spaced\", (0, 1, 110))," >"generator/old dir/gates.rs"
git add -A
git commit -qm base
git tag tools-v0.1.109
base=$(git rev-parse HEAD)
git tag tools-v0.1.110

mkdir -p crates/velnor-gen/src
git mv generator/gates.rs crates/velnor-gen/src/gates.rs
git mv "generator/old dir/gates.rs" crates/velnor-gen/src/spaced.rs
git commit -qm "move files into the gate crates unchanged"
bash "$checker" "$base"
echo 'gate(RustCi, "lint", (0, 1, 110)),' >>crates/velnor-gen/src/gates.rs
git commit -qam "add an unmarked released gate to a moved file"
if bash "$checker" "$base"; then
  echo "the check accepted a new unmarked released gate in a moved file" >&2
  exit 1
fi
