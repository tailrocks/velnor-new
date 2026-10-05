#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/common.sh"

version="${VELNOR_RELEASE_VERSION:?VELNOR_RELEASE_VERSION is required}"
tag="${VELNOR_RELEASE_TAG:?VELNOR_RELEASE_TAG is required}"
repository="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
commit="${GITHUB_SHA:?GITHUB_SHA is required}"
linux_dir="${LINUX_ASSET_DIR:-linux-assets}"
macos_dir="${MACOS_ASSET_DIR:-macos-assets}"
manifest_dir="${MANIFEST_ASSET_DIR:-manifest-assets}"
manifest_name='velnor-actions-release-manifest.json'

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail 'invalid release version'
[[ "$tag" == "v$version" ]] || fail 'release tag does not match version'
[[ "$repository" == "$VELNOR_RELEASE_REPOSITORY" ]] || fail 'unexpected repository'
is_sha "$commit" || fail 'invalid source commit'

workspace_version="$(awk '
  /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
  /^\[/ { in_workspace_package = 0 }
  in_workspace_package && /^version[[:space:]]*=/ {
    gsub(/"/, "", $3)
    print $3
    exit
  }
' Cargo.toml)"
[[ "$workspace_version" == "$version" ]] || fail 'release version differs from workspace package version'

linux_binary="velnor-actions-${version}-x86_64-unknown-linux-gnu"
linux_sidecar="${linux_binary}.sha256"
macos_binary="velnor-actions-${version}-aarch64-apple-darwin"
macos_sidecar="${macos_binary}.sha256"
linux_sha="$(verify_sidecar "$linux_dir" "$linux_sidecar" "$linux_binary")"
macos_sha="$(verify_sidecar "$macos_dir" "$macos_sidecar" "$macos_binary")"

mkdir -p -- "$manifest_dir"
manifest_path="$manifest_dir/$manifest_name"
[[ ! -e "$manifest_path" && ! -L "$manifest_path" ]] \
  || fail 'refusing to overwrite an existing release manifest'
manifest_tmp="$(mktemp "$manifest_dir/.manifest.XXXXXX")" \
  || fail 'could not reserve manifest output'
trap 'rm -f -- "$manifest_tmp"' EXIT
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
[[ -s "$manifest_tmp" ]] || fail 'release manifest is empty'
ln -- "$manifest_tmp" "$manifest_path" \
  || fail 'refusing to replace an existing release manifest'
rm -f -- "$manifest_tmp"
trap - EXIT
