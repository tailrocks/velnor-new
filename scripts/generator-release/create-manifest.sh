#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'generator release manifest: %s\n' "$1" >&2
  exit 1
}

version="${VELNOR_RELEASE_VERSION:?VELNOR_RELEASE_VERSION is required}"
repository="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
commit="${GITHUB_SHA:?GITHUB_SHA is required}"
linux_dir="${LINUX_ASSET_DIR:-linux-assets}"
macos_dir="${MACOS_ASSET_DIR:-macos-assets}"
manifest_dir="${MANIFEST_ASSET_DIR:-manifest-assets}"
manifest_name="velnor-actions-release-manifest.json"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail 'invalid release version'
[[ "$repository" == 'tailrocks/velnor-new' ]] || fail 'unexpected repository'
[[ "$commit" =~ ^[0-9a-f]{40}$ ]] || fail 'invalid source commit'

linux_binary="velnor-actions-${version}-x86_64-unknown-linux-gnu"
linux_sidecar="${linux_binary}.sha256"
macos_binary="velnor-actions-${version}-aarch64-apple-darwin"
macos_sidecar="${macos_binary}.sha256"
tag="v${version}"

verify_sidecar() {
  local directory="$1" sidecar="$2" binary="$3" digest name extra
  [[ -s "$directory/$binary" && -s "$directory/$sidecar" ]] || fail "missing asset: $binary"
  (cd "$directory" && shasum -a 256 -c "$sidecar") >/dev/null || fail "checksum mismatch: $binary"
  [[ "$(wc -l < "$directory/$sidecar" | tr -d ' ')" == 1 ]] || fail "malformed checksum: $sidecar"
  read -r digest name extra < "$directory/$sidecar" || fail "malformed checksum: $sidecar"
  [[ -z "${extra:-}" && "$name" == "$binary" ]] || fail "unexpected checksum filename: $sidecar"
  [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || fail "malformed checksum digest: $sidecar"
  printf '%s' "$digest"
}

linux_sha="$(verify_sidecar "$linux_dir" "$linux_sidecar" "$linux_binary")"
macos_sha="$(verify_sidecar "$macos_dir" "$macos_sidecar" "$macos_binary")"
mkdir -p "$manifest_dir"
manifest_path="$manifest_dir/$manifest_name"
manifest_tmp="${manifest_path}.tmp"

jq -cnS \
  --arg version "$version" \
  --arg repository "$repository" \
  --arg commit "$commit" \
  --arg linux_target 'x86_64-unknown-linux-gnu' \
  --arg linux_url "https://github.com/${repository}/releases/download/${tag}/${linux_binary}" \
  --arg linux_sha "$linux_sha" \
  --arg macos_target 'aarch64-apple-darwin' \
  --arg macos_url "https://github.com/${repository}/releases/download/${tag}/${macos_binary}" \
  --arg macos_sha "$macos_sha" \
  '{schema:1,version:$version,repository:$repository,commit:$commit,targets:[{target:$linux_target,artifact:$linux_url,sha256:$linux_sha},{target:$macos_target,artifact:$macos_url,sha256:$macos_sha}]}' \
  > "$manifest_tmp" || fail 'could not encode release manifest'
mv "$manifest_tmp" "$manifest_path"
