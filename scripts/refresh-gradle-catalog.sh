#!/usr/bin/env bash
set -euo pipefail

base="https://services.gradle.org/distributions"
[ "$#" -gt 0 ] || { echo "usage: $0 <version>..." >&2; exit 2; }

fetch() {
  local digest
  digest="$(curl --fail --silent --show-error --location "$1" | tr -d '[:space:]')"
  [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || { echo "bad digest from $1" >&2; exit 1; }
  printf '%s' "$digest"
}

for version in "$@"; do
  [[ "$version" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] || { echo "bad version $version" >&2; exit 2; }
  printf '    GradleRelease {\n        version: "%s",\n' "$version"
  printf '        bin_sha256: "%s",\n' "$(fetch "$base/gradle-$version-bin.zip.sha256")"
  printf '        all_sha256: "%s",\n' "$(fetch "$base/gradle-$version-all.zip.sha256")"
  printf '        wrapper_jar_sha256: "%s",\n    },\n' "$(fetch "$base/gradle-$version-wrapper.jar.sha256")"
done
