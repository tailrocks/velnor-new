#!/usr/bin/env bash
# Print `GradleRelease` rows for crates/velnor-actions-gradle/src/catalog.rs
# straight from the official services.gradle.org `.sha256` files, so digests
# are never typed by hand. Usage: refresh-gradle-catalog.sh <version>...
# Pass the final (non-snapshot, non-rc) versions from
# https://services.gradle.org/versions/all.
set -euo pipefail

base="https://services.gradle.org/distributions"
[ "$#" -gt 0 ] || { echo "usage: $0 <version>..." >&2; exit 2; }

fetch() {
  local digest
  if ! digest="$(curl --fail --silent --show-error --location "$1" | tr -d '[:space:]')"; then
    echo "The script could not fetch the digest from $1." >&2
    return 1
  fi
  [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || { echo "bad digest from $1" >&2; return 1; }
  printf '%s' "$digest"
}

for version in "$@"; do
  [[ "$version" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] || { echo "bad version $version" >&2; exit 2; }
  bin_sha256="$(fetch "$base/gradle-$version-bin.zip.sha256")" || exit 1
  all_sha256="$(fetch "$base/gradle-$version-all.zip.sha256")" || exit 1
  wrapper_jar_sha256="$(fetch "$base/gradle-$version-wrapper.jar.sha256")" || exit 1
  printf '    GradleRelease {\n        version: "%s",\n' "$version"
  printf '        bin_sha256: "%s",\n' "$bin_sha256"
  printf '        all_sha256: "%s",\n' "$all_sha256"
  printf '        wrapper_jar_sha256: "%s",\n    },\n' "$wrapper_jar_sha256"
done
