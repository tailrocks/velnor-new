#!/bin/sh
# Builds the git world a resolve-qualification case runs in, with fixed identities and dates so
# every commit id is the same on every machine. Usage: repo.sh <new directory>
# Branches: main = base (A) then source (B); side = a commit off A that main does not contain (C);
# many = B plus 300 files (D).
set -eu
dir=$1
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.com GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.com
export GIT_AUTHOR_DATE=2026-01-01T00:00:00Z GIT_COMMITTER_DATE=2026-01-01T00:00:00Z
mkdir -p "$dir"
cd "$dir"
git init -q -b main .
git config commit.gpgsign false
mkdir -p .velnor src docs
printf 'schema = 1\n\n[workflow]\npolicy = "consumer-v1"\ndefault_branch = "main"\n' > .velnor/config.toml
echo a > src/lib.rs
echo d > docs/readme.md
git add -A
git commit -q -m base
git checkout -q -b side
echo c > side.txt
git add -A
git commit -q -m side
git checkout -q main
echo b >> src/lib.rs
echo n > src/new.rs
echo e > "docs/with space.md"
git add -A
git commit -q -m source
git checkout -q -b many
mkdir many
i=0
while [ "$i" -lt 300 ]; do echo "$i" > "many/f$i.txt"; i=$((i + 1)); done
git add -A
git commit -q -m many
git checkout -q main
