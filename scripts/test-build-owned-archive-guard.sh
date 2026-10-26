#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repository="$(cd -- "$script_dir/.." && pwd -P)"
test_root="${HOME:?}/.cache/velnor/archive-guard-target-regression"
mkdir -p -- "$test_root"
chmod 700 "$test_root"
case_root="$(mktemp -d "$test_root/run.XXXXXX")"
cleanup() { rm -rf -- "$case_root"; }
trap cleanup EXIT

case_home="$case_root/home"
tool_bin="$case_root/tool-bin"
mkdir -m 700 -- "$case_home" "$tool_bin"
cp -- "$repository/.mise-version" "$tool_bin/pinned-mise-version"

copy_checkout() {
  local destination="$1"
  mkdir -p -- "$destination/.velnor" "$destination/scripts" \
    "$destination/crates/velnor-archive-guard/src" \
    "$destination/crates/velnor-archive-guard/build_support"
  cp -- "$repository/Cargo.toml" "$repository/Cargo.lock" "$repository/.mise-version" \
    "$destination/"
  cp -- "$repository/.velnor/version-policy.toml" "$destination/.velnor/"
  cp -- "$repository/scripts/archive-guard-inputs.txt" \
    "$repository/scripts/archive-guard-build-namespace.sh" \
    "$repository/scripts/build-owned-archive-guard.sh" \
    "$repository/scripts/check-owned-archive-guard-sources.sh" "$destination/scripts/"
  cp -- "$repository/crates/velnor-archive-guard/Cargo.toml" \
    "$repository/crates/velnor-archive-guard/build.rs" \
    "$destination/crates/velnor-archive-guard/"
  cp -- "$repository/crates/velnor-archive-guard/build_support/archive_guard_inputs.rs" \
    "$repository/crates/velnor-archive-guard/build_support/archive_guard_inputs_filesystem.rs" \
    "$destination/crates/velnor-archive-guard/build_support/"
  cp -R -- "$repository/crates/velnor-archive-guard/src/." \
    "$destination/crates/velnor-archive-guard/src/"
}

copy_checkout "$case_root/checkout-a"
copy_checkout "$case_root/checkout-b"
source_a="$case_root/checkout-a/crates/velnor-archive-guard/src/bin/velnor_archive_guard.rs"
source_b="$case_root/checkout-b/crates/velnor-archive-guard/src/bin/velnor_archive_guard.rs"
touch -r "$source_a" "$source_b"
mkdir -m 700 -- "$case_root/checkout-b/scripts/__pycache__"
printf 'malformed executable cache sentinel\n' \
  >"$case_root/checkout-b/scripts/__pycache__/owned_archive_preflight.cpython-313.pyc"
cat >"$tool_bin/python3" <<'NO_PYTHON'
#!/usr/bin/env bash
printf 'unexpected interpreter invocation\n' >>"$(dirname -- "$0")/python-invocations"
exit 97
NO_PYTHON
chmod 700 "$tool_bin/python3"
cp -- "${VELNOR_ARCHIVE_GUARD_TEST_NATIVE:?native archive guard test binary is required}" \
  "$tool_bin/native-guard"
chmod 700 "$tool_bin/native-guard"

cat >"$tool_bin/mise" <<'FAKE_MISE'
#!/usr/bin/env bash
set -euo pipefail

tool_state="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
private_root="$(cd -- "${HOME:?}" && pwd -P)/.cache/velnor/archive-guard"
for variable in MISE_DATA_DIR MISE_CACHE_DIR MISE_STATE_DIR MISE_CARGO_HOME CARGO_HOME RUSTUP_HOME; do
  value="${!variable:-}"
  [[ "$value" == "$private_root/"* ]] \
    || { printf '%s escaped the private build namespace\n' "$variable" >&2; exit 98; }
  printf '%s=%s\n' "$variable" "$value" >>"$tool_state/mise-environments"
done
for variable in CARGO_TARGET_DIR TMPDIR; do
  value="${!variable:-}"
  if [[ -n "$value" ]]; then
    [[ "$value" == "$private_root/"* ]] \
      || { printf '%s escaped the private build namespace\n' "$variable" >&2; exit 98; }
    printf '%s=%s\n' "$variable" "$value" >>"$tool_state/mise-environments"
  fi
done
while [[ "${1:-}" == --* ]]; do shift; done
command_name="${1:-}"
[[ -n "$command_name" ]] || exit 2
shift

case "$command_name" in
  version)
    cat "$tool_state/pinned-mise-version"
    ;;
  install)
    [[ "${1:-}" == rust@1.99.1 ]]
    ;;
  exec)
    [[ "${1:-}" == rust@1.99.1 ]] || exit 2
    shift
    [[ "${1:-}" == -- ]] || exit 2
    shift
    tool="${1:-}"
    shift || true
    case "$tool" in
      rustc)
        if [[ "${1:-}" == --version ]]; then
          printf 'rustc 1.99.1 (0123456789abcdef 2026-01-01)\n'
        elif [[ "${1:-}" == -vV ]]; then
          printf 'rustc 1.99.1 (0123456789abcdef 2026-01-01)\n'
          printf 'binary: rustc\ncommit-hash: 0123456789abcdef\n'
          printf 'commit-date: 2026-01-01\nhost: x86_64-unknown-linux-gnu\n'
          printf 'release: 1.99.1\nLLVM version: 21.1.8\n'
        else
          exit 2
        fi
        ;;
      cargo)
        case "${1:-}" in
          --version)
            printf 'cargo 1.99.1 (abcdef0123456789 2026-01-01)\n'
            ;;
          tree)
            manifest=''
            while (($#)); do
              if [[ "$1" == --manifest-path ]]; then
                shift
                manifest="${1:-}"
              fi
              shift
            done
            [[ -n "$manifest" ]] || exit 2
            checkout="$(dirname -- "$manifest")"
            printf 'velnor-archive-guard v0.1.0 (%s/crates/velnor-archive-guard)\n' \
              "$checkout"
            ;;
          build)
            target="${CARGO_TARGET_DIR:?}"
            manifest=''
            while (($#)); do
              if [[ "$1" == --manifest-path ]]; then
                shift
                manifest="${1:-}"
              fi
              shift
            done
            [[ -n "$manifest" ]] || exit 2
            checkout="$(dirname -- "$manifest")"
            artifact="$target/release/velnor-archive-guard"
            mkdir -p -- "$(dirname -- "$artifact")"
            printf '%s\n' "$target" >>"$tool_state/build-targets"
            repository="$checkout"
            fingerprint="$("$tool_state/native-guard" source-fingerprint "$checkout")"
            cat >"$artifact" <<EOF
#!/usr/bin/env bash
case "\${1:-}" in
  --fingerprint) printf '%s\\n' '$fingerprint' ;;
  source-fingerprint) exec "$tool_state/native-guard" source-fingerprint "\${2:?}" ;;
  *) exit 2 ;;
esac
EOF
            chmod 755 "$artifact"
            ;;
          *) exit 2 ;;
        esac
        ;;
      *) exit 2 ;;
    esac
    ;;
  *) exit 2 ;;
esac
FAKE_MISE
chmod 700 "$tool_bin/mise"

run_builder() {
  local checkout="$1"
  HOME="$case_home" MISE_STATE_DIR="$case_root/ambient-mise-state" \
    PATH="$tool_bin:/usr/bin:/bin" \
    bash "$checkout/scripts/build-owned-archive-guard.sh"
}

fingerprint_a="$("$tool_bin/native-guard" source-fingerprint "$case_root/checkout-a")"
fingerprint_b="$("$tool_bin/native-guard" source-fingerprint "$case_root/checkout-b")"
[[ "$fingerprint_a" == "$fingerprint_b" ]] \
  || { printf 'identical source fingerprints disagree\n' >&2; exit 1; }
run_builder "$case_root/checkout-a"
run_builder "$case_root/checkout-b"
guard_b="$case_root/checkout-b/.velnor/archive-guard/bin/velnor-archive-guard"
[[ "$("$guard_b" --fingerprint)" == "$fingerprint_a" ]] \
  || { printf 'native bootstrap produced a mismatched guard binary\n' >&2; exit 1; }
printf '\n// byte change with preserved mtime\n' >>"$source_b"
touch -r "$source_a" "$source_b"
fingerprint_b="$("$tool_bin/native-guard" source-fingerprint "$case_root/checkout-b")"
[[ "$fingerprint_a" != "$fingerprint_b" ]] \
  || { printf 'source bytes did not change the fingerprint\n' >&2; exit 1; }
run_builder "$case_root/checkout-b"

guard_a="$case_root/checkout-a/.velnor/archive-guard/bin/velnor-archive-guard"
[[ "$("$guard_a" --fingerprint)" == "$fingerprint_a" ]] \
  || { printf 'checkout A binary changed or mismatched\n' >&2; exit 1; }
[[ "$("$guard_b" --fingerprint)" == "$fingerprint_b" ]] \
  || { printf 'checkout B installed a stale binary\n' >&2; exit 1; }

targets_file="$tool_bin/build-targets"
target_count="$(wc -l <"$targets_file" | tr -d ' ')"
[[ "$target_count" == 6 ]] \
  || { printf 'unexpected stage-zero/final Cargo build count\n' >&2; exit 1; }
bootstrap_a="$(sed -n '1p' "$targets_file")"
target_a="$(sed -n '2p' "$targets_file")"
bootstrap_same="$(sed -n '3p' "$targets_file")"
target_same="$(sed -n '4p' "$targets_file")"
bootstrap_changed="$(sed -n '5p' "$targets_file")"
target_changed="$(sed -n '6p' "$targets_file")"
[[ -n "$target_a" && "$target_a" == "$target_same" \
  && -n "$target_changed" && "$target_a" != "$target_changed" \
  && "$bootstrap_a" != "$bootstrap_same" && "$bootstrap_same" != "$bootstrap_changed" \
  && "$bootstrap_a" != "$target_a" && "$bootstrap_same" != "$target_same" ]] \
  || { printf 'Cargo target namespace was shared across changed source bytes\n' >&2; exit 1; }
[[ "$target_a" == "$case_home/.cache/velnor/archive-guard/target/$fingerprint_a-"* \
  && "$target_changed" == "$case_home/.cache/velnor/archive-guard/target/$fingerprint_b-"* ]] \
  || { printf 'Cargo target namespace is not keyed by the verified source fingerprint\n' >&2; exit 1; }
[[ ! -e "$tool_bin/python-invocations" ]] \
  || { printf 'archive guard invoked a Python interpreter\n' >&2; exit 1; }
[[ -s "$tool_bin/mise-environments" ]] \
  || { printf 'Mise environment isolation was not checked\n' >&2; exit 1; }
expected_private_root="$(cd -- "$case_home" && pwd -P)/.cache/velnor/archive-guard"
while IFS='=' read -r variable value; do
  [[ "$value" == "$expected_private_root/"* ]] \
    || { printf 'Mise environment escaped private namespace: %s\n' "$variable" >&2; exit 1; }
done <"$tool_bin/mise-environments"
private_state="$expected_private_root/mise-state"
state_count="$(grep -c '^MISE_STATE_DIR=' "$tool_bin/mise-environments")"
data_count="$(grep -c '^MISE_DATA_DIR=' "$tool_bin/mise-environments")"
[[ "$private_state" != "$case_root/ambient-mise-state" \
  && "$state_count" -gt 0 && "$state_count" == "$data_count" ]] \
  || { printf 'Mise state was not isolated from the caller environment\n' >&2; exit 1; }
printf 'archive guard target and native bootstrap regression passed\n'
