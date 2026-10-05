#!/usr/bin/env bash
set -euo pipefail

version="${1:?version is required}"
repository="${2:?repository is required}"
rust_version="${3:?Rust toolchain version is required}"
mr_boxington_version="${4:?MBX tool version is required}"
readonly expected_repository="tailrocks/velnor-new"

if [[ "$repository" != "$expected_repository" || "$GITHUB_REPOSITORY" != "$expected_repository" ]]; then
  echo "unexpected release repository" >&2
  exit 1
fi
if [[ ! "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]]; then
  echo "release source SHA is malformed" >&2
  exit 1
fi
if [[ ! "$rust_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ || ! "$mr_boxington_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "release toolchain versions are malformed" >&2
  exit 1
fi

verify_asset() {
  local target="$1" directory="$2" digest_name="$3"
  local binary="velnor-actions-${version}-${target}"
  local sidecar="${binary}.sha256"
  local provenance="${binary}.provenance.json"
  local digest
  digest="$(strict_sidecar_digest "$directory/$sidecar" "$binary")"
  jq -e --arg version "$version" --arg repository "$repository" \
    --arg commit "$GITHUB_SHA" --arg target "$target" --arg asset "$binary" \
    --arg sha256 "$digest" --arg rust "$rust_version" \
    --arg mr_boxington "$mr_boxington_version" \
    '.schema == 1 and .version == $version and .repository == $repository and
     .commit == $commit and .target == $target and .asset == $asset and
     .sha256 == $sha256 and .toolchain.rust == $rust and
     .toolchain["mr-boxington"] == $mr_boxington' \
    "$directory/$provenance" >/dev/null
  (cd "$directory" && sha256sum --check "$sidecar")
  printf -v "$digest_name" '%s' "$digest"
}

strict_sidecar_digest() {
  local sidecar="$1" binary="$2"
  awk -v expected="$binary" '
    NR == 1 {
      if (NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ || $2 != expected) exit 1
      print $1
      next
    }
    { exit 1 }
    END { if (NR != 1) exit 1 }
  ' "$sidecar"
}

verify_asset x86_64-unknown-linux-gnu linux-assets linux_sha256
verify_asset aarch64-apple-darwin macos-assets macos_arm64_sha256

tag="v${version}"
jq -n --arg version "$version" --arg repository "$repository" \
  --arg commit "$GITHUB_SHA" --arg tag "$tag" \
  --arg linux_sha256 "$linux_sha256" --arg macos_arm64_sha256 "$macos_arm64_sha256" '
  {schema:1, version:$version, repository:$repository, commit:$commit,
   targets:[
     {target:"x86_64-unknown-linux-gnu",
      artifact:("https://github.com/" + $repository + "/releases/download/" + $tag + "/velnor-actions-" + $version + "-x86_64-unknown-linux-gnu"),
      sha256:$linux_sha256},
     {target:"aarch64-apple-darwin",
      artifact:("https://github.com/" + $repository + "/releases/download/" + $tag + "/velnor-actions-" + $version + "-aarch64-apple-darwin"),
      sha256:$macos_arm64_sha256}
   ]}
' > release-manifest.json
jq -e --arg version "$version" --arg repository "$repository" \
  --arg commit "$GITHUB_SHA" --arg tag "$tag" \
  '.schema == 1 and .version == $version and .repository == $repository and
   .commit == $commit and (.targets | length) == 2 and
   .targets[0].target == "x86_64-unknown-linux-gnu" and
   .targets[0].artifact == ("https://github.com/" + $repository + "/releases/download/" + $tag + "/velnor-actions-" + $version + "-x86_64-unknown-linux-gnu") and
   (.targets[0].sha256 | test("^[0-9a-f]{64}$")) and
   .targets[1].target == "aarch64-apple-darwin" and
   .targets[1].artifact == ("https://github.com/" + $repository + "/releases/download/" + $tag + "/velnor-actions-" + $version + "-aarch64-apple-darwin") and
   (.targets[1].sha256 | test("^[0-9a-f]{64}$"))' \
  release-manifest.json >/dev/null
test -s release-manifest.json
