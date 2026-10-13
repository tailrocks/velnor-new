#!/usr/bin/env bash
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
    "$repository/scripts/check-owned-archive-guard-sources.sh" \
    "$repository/scripts/owned_archive_preflight.py" "$destination/scripts/"
  cp -- "$repository/crates/velnor-archive-guard/Cargo.toml" \
    "$repository/crates/velnor-archive-guard/build.rs" \
    "$destination/crates/velnor-archive-guard/"
  cp -- "$repository/crates/velnor-archive-guard/build_support/archive_guard_inputs.rs" \
    "$destination/crates/velnor-archive-guard/build_support/"
  cp -R -- "$repository/crates/velnor-archive-guard/src/." \
    "$destination/crates/velnor-archive-guard/src/"
}

copy_checkout "$case_root/checkout-a"
copy_checkout "$case_root/checkout-b"
source_a="$case_root/checkout-a/crates/velnor-archive-guard/src/bin/velnor_archive_guard.rs"
source_b="$case_root/checkout-b/crates/velnor-archive-guard/src/bin/velnor_archive_guard.rs"
shared_mtime="$(python3 - "$source_a" "$source_b" <<'PY'
import os
import sys
first = os.stat(sys.argv[1])
second = os.stat(sys.argv[2])
os.utime(sys.argv[2], ns=(second.st_atime_ns, first.st_mtime_ns))
if os.stat(sys.argv[1]).st_mtime_ns != os.stat(sys.argv[2]).st_mtime_ns:
    raise SystemExit("checkout source mtimes do not match")
print(first.st_mtime_ns)
PY
)"
trap_fingerprint='ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'
python3 - "$case_root/checkout-b/scripts/owned_archive_preflight.py" \
  "$trap_fingerprint" <<'PY'
import importlib.util
import marshal
import os
from pathlib import Path
import struct
import sys

source = Path(sys.argv[1])
metadata = source.stat()
cache = Path(importlib.util.cache_from_source(str(source)))
cache.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
code = compile(
    "def _source_fingerprint(repository):\n    return " + repr(sys.argv[2]) + "\n",
    str(source), "exec")
cache_bytes = (importlib.util.MAGIC_NUMBER + struct.pack("<I", 0)
               + struct.pack("<II", metadata.st_mtime_ns // 1_000_000_000,
                             metadata.st_size)
               + marshal.dumps(code))
cache.write_bytes(cache_bytes)
os.chmod(cache, 0o600)
PY
imported_fingerprint="$(python3 -B -c '
import sys
sys.path.insert(0, sys.argv[1])
from owned_archive_preflight import _source_fingerprint
print(_source_fingerprint(None))
' "$case_root/checkout-b/scripts")"
[[ "$imported_fingerprint" == "$trap_fingerprint" ]] \
  || { printf 'trap bytecode was not valid for the source fixture\n' >&2; exit 1; }

cat >"$tool_bin/mise" <<'FAKE_MISE'
#!/usr/bin/env bash
set -euo pipefail

tool_state="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
while [[ "${1:-}" == --* ]]; do shift; done
command_name="${1:-}"
[[ -n "$command_name" ]] || exit 2
shift

case "$command_name" in
  version)
    printf '2026.10.6\n'
    ;;
  install)
    [[ "${1:-}" == rust@1.98.1 ]]
    ;;
  exec)
    [[ "${1:-}" == rust@1.98.1 ]] || exit 2
    shift
    [[ "${1:-}" == -- ]] || exit 2
    shift
    tool="${1:-}"
    shift || true
    case "$tool" in
      rustc)
        if [[ "${1:-}" == --version ]]; then
          printf 'rustc 1.98.1 (0123456789abcdef 2026-01-01)\n'
        elif [[ "${1:-}" == -vV ]]; then
          printf 'rustc 1.98.1 (0123456789abcdef 2026-01-01)\n'
          printf 'binary: rustc\ncommit-hash: 0123456789abcdef\n'
          printf 'commit-date: 2026-01-01\nhost: x86_64-unknown-linux-gnu\n'
          printf 'release: 1.98.1\nLLVM version: 21.1.8\n'
        else
          exit 2
        fi
        ;;
      cargo)
        case "${1:-}" in
          --version)
            printf 'cargo 1.98.1 (abcdef0123456789 2026-01-01)\n'
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
            if [[ ! -e "$artifact" ]]; then
              repository="$checkout"
              fail() { printf 'fake Mise: %s\n' "$1" >&2; exit 1; }
              # shellcheck source=scripts/archive-guard-build-namespace.sh
              source "$checkout/scripts/archive-guard-build-namespace.sh"
              fingerprint="$(archive_guard_source_fingerprint)"
              cat >"$artifact" <<EOF
#!/usr/bin/env bash
printf '%s\\n' '$fingerprint'
EOF
              chmod 755 "$artifact"
            fi
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
  HOME="$case_home" PATH="$tool_bin:/usr/bin:/bin" \
    bash "$checkout/scripts/build-owned-archive-guard.sh"
}

fingerprint_for() (
  repository="$1"
  fail() { printf 'archive guard regression: %s\n' "$1" >&2; exit 1; }
  # shellcheck source=scripts/archive-guard-build-namespace.sh
  source "$repository/scripts/archive-guard-build-namespace.sh"
  archive_guard_source_fingerprint
)

fingerprint_a="$(fingerprint_for "$case_root/checkout-a")"
fingerprint_b="$(fingerprint_for "$case_root/checkout-b")"
[[ "$fingerprint_a" == "$fingerprint_b" && "$fingerprint_b" != "$trap_fingerprint" ]] \
  || { printf 'identical source prehash disagrees or executed trap bytecode\n' >&2; exit 1; }
run_builder "$case_root/checkout-a"
run_builder "$case_root/checkout-b"
guard_b="$case_root/checkout-b/.velnor/archive-guard/bin/velnor-archive-guard"
[[ "$("$guard_b" --fingerprint)" == "$fingerprint_a" ]] \
  || { printf 'builder prehash executed or trusted trap bytecode\n' >&2; exit 1; }
original_times="$(python3 - "$source_b" <<'PY'
import os
import sys
metadata = os.stat(sys.argv[1])
print(metadata.st_atime_ns, metadata.st_mtime_ns)
PY
)"
printf '\n// byte change with preserved mtime\n' >>"$source_b"
python3 - "$source_b" "$original_times" "$shared_mtime" <<'PY'
import os
import sys
atime_ns, _ = map(int, sys.argv[2].split())
mtime_ns = int(sys.argv[3])
os.utime(sys.argv[1], ns=(atime_ns, mtime_ns))
metadata = os.stat(sys.argv[1])
if metadata.st_mtime_ns != mtime_ns:
    raise SystemExit("source mtime was not preserved")
PY
fingerprint_b="$(fingerprint_for "$case_root/checkout-b")"
[[ "$fingerprint_a" != "$fingerprint_b" ]] \
  || { printf 'source bytes did not change the fingerprint\n' >&2; exit 1; }
run_builder "$case_root/checkout-b"

guard_a="$case_root/checkout-a/.velnor/archive-guard/bin/velnor-archive-guard"
[[ "$("$guard_a" --fingerprint)" == "$fingerprint_a" ]] \
  || { printf 'checkout A binary changed or mismatched\n' >&2; exit 1; }
[[ "$("$guard_b" --fingerprint)" == "$fingerprint_b" ]] \
  || { printf 'checkout B installed a stale binary\n' >&2; exit 1; }

targets_file="$tool_bin/build-targets"
target_a=''
target_same=''
target_changed=''
target_count=0
while IFS= read -r target; do
  case "$target_count" in
    0) target_a="$target" ;;
    1) target_same="$target" ;;
    2) target_changed="$target" ;;
    *) printf 'unexpected fake Cargo build count\n' >&2; exit 1 ;;
  esac
  target_count=$((target_count + 1))
done <"$targets_file"
[[ "$target_count" == 3 && -n "$target_a" && "$target_a" == "$target_same" \
  && -n "$target_changed" && "$target_a" != "$target_changed" ]] \
  || { printf 'Cargo target namespace was shared across changed source bytes\n' >&2; exit 1; }
[[ "$target_a" == "$case_home/.cache/velnor/archive-guard/target/$fingerprint_a-"* \
  && "$target_changed" == "$case_home/.cache/velnor/archive-guard/target/$fingerprint_b-"* ]] \
  || { printf 'Cargo target namespace is not keyed by the verified source fingerprint\n' >&2; exit 1; }
printf 'archive guard target and bytecode regression passed\n'
