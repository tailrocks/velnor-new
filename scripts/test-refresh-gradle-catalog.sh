#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
script="$script_dir/refresh-gradle-catalog.sh"
test_root="$(mktemp -d)"
trap 'rm -rf -- "$test_root"' EXIT
tool_bin="$test_root/bin"
mkdir -m 700 "$tool_bin"

cat >"$tool_bin/curl" <<'CURL'
#!/usr/bin/env bash
set -euo pipefail
url="${@: -1}"
name="${url##*/}"
case "${MOCK_CURL_MODE:-success}:$name" in
  fail:*)
    if [[ "$name" == "gradle-$MOCK_CURL_MODE_SUFFIX" ]]; then
      exit 22
    fi
    ;;
  malformed:*)
    if [[ "$name" == "gradle-$MOCK_CURL_MODE_SUFFIX" ]]; then
      printf 'not-a-sha256\n'
      exit 0
    fi
    ;;
esac
case "$name" in
  *-bin.zip.sha256) printf '%064d\n' 1 ;;
  *-all.zip.sha256) printf '%064d\n' 2 ;;
  *-wrapper.jar.sha256) printf '%064d\n' 3 ;;
  *) exit 2 ;;
esac
CURL
chmod 700 "$tool_bin/curl"

expected_bin="$(printf '%064d' 1)"
expected_all="$(printf '%064d' 2)"
expected_wrapper="$(printf '%064d' 3)"
expected="    GradleRelease {
        version: \"9.8.2\",
        bin_sha256: \"$expected_bin\",
        all_sha256: \"$expected_all\",
        wrapper_jar_sha256: \"$expected_wrapper\",
    },"
actual="$(PATH="$tool_bin:/usr/bin:/bin" bash "$script" 9.8.2)"
[[ "$actual" == "$expected" ]] || {
  printf 'The checksum refresh produced unexpected rows.\n' >&2
  exit 1
}

for kind in bin all wrapper; do
  case "$kind" in
    bin) suffix="9.8.2-bin.zip.sha256" ;;
    all) suffix="9.8.2-all.zip.sha256" ;;
    wrapper) suffix="9.8.2-wrapper.jar.sha256" ;;
  esac
  if output="$(PATH="$tool_bin:/usr/bin:/bin" MOCK_CURL_MODE=fail MOCK_CURL_MODE_SUFFIX="$suffix" bash "$script" 9.8.2 2>"$test_root/error")"; then
    printf 'The refresh command succeeded after curl failed for %s.\n' "$kind" >&2
    exit 1
  fi
  [[ -z "$output" ]] || {
    printf 'The refresh command wrote a partial row after curl failed for %s.\n' "$kind" >&2
    exit 1
  }
done

for kind in bin all wrapper; do
  case "$kind" in
    bin) suffix="9.8.2-bin.zip.sha256" ;;
    all) suffix="9.8.2-all.zip.sha256" ;;
    wrapper) suffix="9.8.2-wrapper.jar.sha256" ;;
  esac
  if output="$(PATH="$tool_bin:/usr/bin:/bin" MOCK_CURL_MODE=malformed MOCK_CURL_MODE_SUFFIX="$suffix" bash "$script" 9.8.2 2>"$test_root/error")"; then
    printf 'The refresh command succeeded with an invalid checksum for %s.\n' "$kind" >&2
    exit 1
  fi
  [[ -z "$output" ]] || {
    printf 'The refresh command wrote a partial row with an invalid checksum for %s.\n' "$kind" >&2
    exit 1
  }
done
