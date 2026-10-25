#!/usr/bin/env bash
# Exercise capture/check producer parity in an isolated thin repository.
set -euo pipefail

SOURCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/velnor-dogfood-parity-test.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

TEST_ROOT="$WORK/repo"
SCRIPT="$TEST_ROOT/scripts/capture-opentofu-goldens.sh"
GOLDEN_DIR="$TEST_ROOT/docs/proposed/opentofu-goldens"
FAKE_CLI="$WORK/fake-velnor-actions"
COLLECTOR_TMP="$WORK/collector tmp"
mkdir -p \
  "$TEST_ROOT/scripts/generator-release" \
  "$TEST_ROOT/.github/workflows" \
  "$TEST_ROOT/fixtures/nested" \
  "$TEST_ROOT/fixtures/mbx-nextest" \
  "$TEST_ROOT/fixtures/empty-suite" \
  "$TEST_ROOT/fixtures/minimal-cargo" \
  "$GOLDEN_DIR" \
  "$COLLECTOR_TMP"
cp "$SOURCE_ROOT/scripts/capture-opentofu-goldens.sh" "$SCRIPT"
chmod u+x "$SCRIPT"

cat >"$TEST_ROOT/scripts/generator-release/qualification-goldens.sh" <<'HELPER'
fixture_git() {
  git "$@"
}
HELPER

for fixture in nested mbx-nextest empty-suite minimal-cargo; do
  printf '%s\n' "$fixture" >"$TEST_ROOT/fixtures/$fixture/input.txt"
done
printf 'generated\n' >"$TEST_ROOT/.github/workflows/ci.yml"

cat >"$FAKE_CLI" <<'CLI'
#!/usr/bin/env bash
set -euo pipefail

case "${1-}" in
  --version)
    printf 'velnor-actions 1.2.3\n'
    ;;
  plan)
    printf 'Repository: %s\n' "$PWD"
    printf 'Plan: fake fixture\n'
    ;;
  generate)
    if [ "${2-}" != "--output-dir" ] || [ -z "${3-}" ]; then
      echo 'unexpected generate arguments' >&2
      exit 64
    fi
    mkdir -p "$3/.github/workflows"
    printf '%s\n' "${VELNOR_TEST_GENERATED_CI:-generated}" \
      >"$3/.github/workflows/ci.yml"
    ;;
  *)
    echo "unexpected CLI command: ${1-}" >&2
    exit 64
    ;;
esac
CLI
chmod u+x "$FAKE_CLI"

cat >"$WORK/gitconfig" <<'GITCONFIG'
[user]
  name = Golden Parity Test
  email = golden-parity@example.invalid
[commit]
  gpgsign = false
[init]
  defaultBranch = main
GITCONFIG
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL="$WORK/gitconfig"
export GIT_AUTHOR_NAME='Golden Parity Test'
export GIT_AUTHOR_EMAIL='golden-parity@example.invalid'
export GIT_COMMITTER_NAME='Golden Parity Test'
export GIT_COMMITTER_EMAIL='golden-parity@example.invalid'
export GIT_AUTHOR_DATE='2026-01-01T00:00:00Z'
export GIT_COMMITTER_DATE='2026-01-01T00:00:00Z'
git -C "$TEST_ROOT" init -q
git -C "$TEST_ROOT" add -A
git -C "$TEST_ROOT" commit -qm 'golden parity fixture'

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

run_collector() {
  local mode="$1" generated="$2" log="$3" status=0
  if TMPDIR="$COLLECTOR_TMP" VELNOR_TEST_GENERATED_CI="$generated" \
    bash "$SCRIPT" "$mode" "$FAKE_CLI" >"$log" 2>&1; then
    status=0
  else
    status=$?
  fi
  return "$status"
}

digest_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  else
    shasum -a 256 | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  fi
}

golden_fingerprint() {
  (
    cd "$GOLDEN_DIR"
    files="$(find . -type f -print | LC_ALL=C sort)" || exit 1
    if [ -n "$files" ]; then
      while IFS= read -r path; do
        if command -v sha256sum >/dev/null 2>&1; then
          sha256sum "$path" || exit 1
        else
          shasum -a 256 "$path" || exit 1
        fi
      done <<<"$files"
    fi
    links="$(find . -type l -print | LC_ALL=C sort)" || exit 1
    if [ -n "$links" ]; then
      while IFS= read -r path; do
        printf 'link %s -> %s\n' "$path" "$(readlink "$path")"
      done <<<"$links"
    fi
  ) | digest_stdin
}

write_manifest() {
  (
    cd "$GOLDEN_DIR/cases"
    files="$(find . -type f -print | LC_ALL=C sort)" || exit 1
    if [ -n "$files" ]; then
      while IFS= read -r path; do
        if command -v sha256sum >/dev/null 2>&1; then
          sha256sum "$path" || exit 1
        else
          shasum -a 256 "$path" || exit 1
        fi
      done <<<"$files"
    fi
    links="$(find . -type l -print | LC_ALL=C sort)" || exit 1
    if [ -n "$links" ]; then
      while IFS= read -r path; do
        printf 'link %s -> %s\n' "$path" "$(readlink "$path")"
      done <<<"$links"
    fi
  ) >"$GOLDEN_DIR/MANIFEST.sha256"
}

if ! run_collector capture generated "$WORK/capture-identical.log"; then
  cat "$WORK/capture-identical.log" >&2
  fail 'capture rejected identical producer workflows'
fi
if [ "$(cat "$GOLDEN_DIR/cases/dogfood/dogfood.verdict")" != identical ]; then
  fail 'identical capture did not record its parity verdict'
fi
if ! run_collector check generated "$WORK/check-identical.log"; then
  cat "$WORK/check-identical.log" >&2
  fail 'check rejected matching goldens and producer workflows'
fi
if ! grep -Fq 'ALL GOLDENS MATCH' "$WORK/check-identical.log"; then
  cat "$WORK/check-identical.log" >&2
  fail 'matching check did not report success'
fi

before="$(golden_fingerprint)" || fail 'could not fingerprint captured goldens'
printf 'shipping\n' >"$TEST_ROOT/.github/workflows/ci.yml"
if run_collector capture generated "$WORK/capture-different.log"; then
  cat "$WORK/capture-different.log" >&2
  fail 'capture accepted differing producer workflows'
fi
if ! grep -Fq 'dogfood generated workflows are not identical' "$WORK/capture-different.log"; then
  cat "$WORK/capture-different.log" >&2
  fail 'differing capture did not report the producer mismatch'
fi
after="$(golden_fingerprint)" || fail 'could not fingerprint captured goldens after mismatch'
if [ "$before" != "$after" ]; then
  fail 'differing capture changed the existing golden tree'
fi

failed_work="$(find "$COLLECTOR_TMP" -mindepth 1 -maxdepth 1 -type d \
  -name 'velnor-goldens-work.*' -print | sed -n '1p')"
if [ -z "$failed_work" ] || [ ! -d "$failed_work/stage/dogfood" ]; then
  cat "$WORK/capture-different.log" >&2
  fail 'failed capture did not retain its isolated comparison stage'
fi

# Model a stale oracle from the old collector: it records the mismatching
# dogfood output itself, including its DIFFERS verdict.
rm -rf "$GOLDEN_DIR/cases/dogfood"
cp -RP "$failed_work/stage/dogfood" "$GOLDEN_DIR/cases/dogfood"
write_manifest || fail 'could not refresh the isolated self-matching test oracle'
if ! diff -r "$GOLDEN_DIR/cases/dogfood" "$failed_work/stage/dogfood" >/dev/null; then
  fail 'self-matching DIFFERS oracle fixture is not byte-identical to its stage'
fi

if run_collector check generated "$WORK/check-different-self-matching.log"; then
  cat "$WORK/check-different-self-matching.log" >&2
  fail 'check accepted a self-matching DIFFERS dogfood oracle'
fi
if ! grep -Fq 'dogfood generated workflows are not identical' \
  "$WORK/check-different-self-matching.log"; then
  cat "$WORK/check-different-self-matching.log" >&2
  fail 'check did not report the self-matching producer mismatch'
fi
if grep -Fq 'ALL GOLDENS MATCH' "$WORK/check-different-self-matching.log"; then
  cat "$WORK/check-different-self-matching.log" >&2
  fail 'check reported success for a DIFFERS dogfood verdict'
fi

echo 'passed dogfood capture/check parity status regression'
