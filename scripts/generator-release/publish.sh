#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'generator release publish: %s\n' "$1" >&2
  exit 1
}

version="${VELNOR_RELEASE_VERSION:?VELNOR_RELEASE_VERSION is required}"
repository="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
commit="${GITHUB_SHA:?GITHUB_SHA is required}"
ref="${GITHUB_REF:?GITHUB_REF is required}"
workflow_ref="${GITHUB_WORKFLOW_REF:?GITHUB_WORKFLOW_REF is required}"
token="${GH_TOKEN:?GH_TOKEN is required}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail 'invalid release version'
[[ "$repository" == 'tailrocks/velnor-new' ]] || fail 'unexpected repository'
[[ "$commit" =~ ^[0-9a-f]{40}$ ]] || fail 'invalid source commit'
[[ -n "$token" ]] || fail 'empty GitHub token'

manifest_name='velnor-actions-release-manifest.json'
linux_binary="velnor-actions-${version}-x86_64-unknown-linux-gnu"
linux_sidecar="${linux_binary}.sha256"
macos_binary="velnor-actions-${version}-aarch64-apple-darwin"
macos_sidecar="${macos_binary}.sha256"
linux_provenance="velnor-actions-${version}-linux-provenance.sigstore.json"
macos_provenance="velnor-actions-${version}-macos-provenance.sigstore.json"
manifest_provenance='velnor-actions-release-manifest.sigstore.json'
tag="v${version}"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

api_get() {
  local path="$1" body="$2" status
  status="$(curl --connect-timeout 5 --max-time 20 --max-filesize 1048576 \
    --silent --show-error --output "$body" --write-out '%{http_code}' \
    --header 'Accept: application/vnd.github+json' \
    --header "Authorization: Bearer ${token}" \
    --header 'X-GitHub-Api-Version: 2022-11-28' \
    "https://api.github.com/repos/${repository}/${path}")" || fail "API request failed: $path"
  printf '%s' "$status"
}

require_success_json() {
  local path="$1" body="$2" status
  status="$(api_get "$path" "$body")"
  [[ "$status" == 200 ]] || fail "API returned HTTP $status: $path"
  jq -e 'type == "object"' "$body" >/dev/null || fail "malformed API response: $path"
}

require_absent() {
  local path="$1" body="$2" status
  status="$(api_get "$path" "$body")"
  if [[ "$status" == 404 ]]; then
    jq -e '.message == "Not Found" and .status == "404"' "$body" >/dev/null \
      || fail "unconfirmed 404 response: $path"
    return
  fi
  if [[ "$status" == 200 ]]; then
    fail "release identity already exists: $path"
  fi
  fail "API returned HTTP $status: $path"
}

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

verify_bundle() {
  local path="$1"
  [[ -s "$path" ]] || fail "missing provenance bundle: $path"
  jq -e '.mediaType | strings | startswith("application/vnd.dev.sigstore.bundle+json")' \
    "$path" >/dev/null || fail "malformed provenance bundle: $path"
  jq -e '.verificationMaterial | type == "object"' "$path" >/dev/null \
    || fail "malformed provenance bundle: $path"
  jq -e '.dsseEnvelope | type == "object"' "$path" >/dev/null \
    || fail "malformed provenance bundle: $path"
}

linux_sha="$(verify_sidecar linux-assets "$linux_sidecar" "$linux_binary")"
macos_sha="$(verify_sidecar macos-assets "$macos_sidecar" "$macos_binary")"
manifest_path="manifest-assets/$manifest_name"
[[ -s "$manifest_path" ]] || fail 'release manifest is missing'
verify_bundle "linux-provenance/$linux_provenance"
verify_bundle "macos-provenance/$macos_provenance"
verify_bundle "manifest-assets/$manifest_provenance"

expected_manifest="$(jq -cnS \
  --arg version "$version" \
  --arg repository "$repository" \
  --arg commit "$commit" \
  --arg linux_target 'x86_64-unknown-linux-gnu' \
  --arg linux_url "https://github.com/${repository}/releases/download/${tag}/${linux_binary}" \
  --arg linux_sha "$linux_sha" \
  --arg macos_target 'aarch64-apple-darwin' \
  --arg macos_url "https://github.com/${repository}/releases/download/${tag}/${macos_binary}" \
  --arg macos_sha "$macos_sha" \
  '{schema:1,version:$version,repository:$repository,commit:$commit,targets:[{target:$linux_target,artifact:$linux_url,sha256:$linux_sha},{target:$macos_target,artifact:$macos_url,sha256:$macos_sha}]}')" \
  || fail 'could not encode expected release manifest'
actual_manifest="$(cat "$manifest_path")"
[[ "$actual_manifest" == "$expected_manifest" ]] || fail 'manifest does not bind exact release assets'

require_success_json "" "$work_dir/repository.json"
default_branch="$(jq -er '.default_branch | strings' "$work_dir/repository.json")" \
  || fail 'malformed repository default branch'
[[ "$default_branch" =~ ^[A-Za-z0-9._-]+$ ]] || fail 'unsupported default branch name'
[[ "$ref" == "refs/heads/${default_branch}" ]] || fail 'release ref is not the default branch'
expected_workflow="${repository}/.github/workflows/generator-release.yml@refs/heads/${default_branch}"
[[ "$workflow_ref" == "$expected_workflow" ]] || fail 'release workflow is not running from the default branch'
require_success_json "branches/${default_branch}" "$work_dir/branch.json"
branch_sha="$(jq -er '.commit.sha | strings' "$work_dir/branch.json")" \
  || fail 'malformed default branch response'
[[ "$branch_sha" == "$commit" ]] || fail 'source commit is not current default branch head'
require_absent "git/ref/tags/${tag}" "$work_dir/tag.json"
require_absent "releases/tags/${tag}" "$work_dir/release.json"

tag_response="$(gh api --method POST "repos/${repository}/git/refs" \
  -f "ref=refs/tags/${tag}" -f "sha=${commit}")" || fail 'tag creation failed; do not retry this version'
jq -e --arg ref "refs/tags/${tag}" --arg sha "$commit" \
  '.ref == $ref and .object.sha == $sha and .object.type == "commit"' \
  <<< "$tag_response" >/dev/null || fail 'tag creation response did not bind the source commit'

gh release create "$tag" -R "$repository" --verify-tag --target "$commit" \
  --title "$tag" --latest --notes "Velnor Actions ${version} built from ${commit}." \
  "linux-assets/$linux_binary" "linux-assets/$linux_sidecar" \
  "macos-assets/$macos_binary" "macos-assets/$macos_sidecar" \
  "$manifest_path" "linux-provenance/$linux_provenance" \
  "macos-provenance/$macos_provenance" "manifest-assets/$manifest_provenance"
