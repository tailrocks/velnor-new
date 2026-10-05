#!/usr/bin/env bash
set -euo pipefail

readonly repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly script_root="$repo_root/scripts/generator-release"
readonly temp_root="$(mktemp -d)"
trap 'rm -rf -- "$temp_root"' EXIT
export PATH="$script_root/test-bin:$PATH"
case_directory=''
case_state=''

fail() { printf 'generator release test: %s\n' "$1" >&2; exit 1; }

copy_checkout() {
  local directory="$1"
  mkdir -p "$directory/scripts/generator-release"
  cp "$repo_root/Cargo.toml" "$directory/Cargo.toml"
  cp "$script_root"/*.sh "$directory/scripts/generator-release/"
}

write_sidecar() {
  local directory="$1" binary="$2"
  (cd "$directory" && sha256sum -- "$binary" > "$binary.sha256")
}

fixture_manifest_test() {
  local directory="$temp_root/manifest"
  mkdir -p "$directory/linux-assets" "$directory/macos-assets"
  copy_checkout "$directory"
  printf 'fixture linux executable bytes\n' > "$directory/linux-assets/velnor-actions-0.1.1-x86_64-unknown-linux-gnu"
  printf 'fixture macos executable bytes\n' > "$directory/macos-assets/velnor-actions-0.1.1-aarch64-apple-darwin"
  write_sidecar "$directory/linux-assets" 'velnor-actions-0.1.1-x86_64-unknown-linux-gnu'
  write_sidecar "$directory/macos-assets" 'velnor-actions-0.1.1-aarch64-apple-darwin'
  (cd "$directory" && env \
    GITHUB_REPOSITORY='tailrocks/velnor-new' \
    GITHUB_SHA='0123456789abcdef0123456789abcdef01234567' \
    VELNOR_RELEASE_VERSION='0.1.1' VELNOR_RELEASE_TAG='v0.1.1' \
    bash scripts/generator-release/create-manifest.sh)
  cmp "$directory/manifest-assets/velnor-actions-release-manifest.json" \
    "$script_root/fixtures/manifest-v0.1.1.json" \
    || fail 'real jq manifest differs from the checked-in v0.1.1 fixture'
  if (cd "$directory" && env \
    GITHUB_REPOSITORY='tailrocks/velnor-new' \
    GITHUB_SHA='0123456789abcdef0123456789abcdef01234567' \
    VELNOR_RELEASE_VERSION='0.1.1' VELNOR_RELEASE_TAG='v0.1.1' \
    bash scripts/generator-release/create-manifest.sh >/dev/null 2>&1); then
    fail 'manifest generator overwrote an existing canonical output'
  fi
}

write_artifact_fixture() {
  local state="$1" assets_json='[]' index
  local -a ids=(501 502 503 504 505 506)
  local -a names=(
    "$VELNOR_LINUX_ARTIFACT_NAME" "$VELNOR_LINUX_BUNDLE_NAME"
    "$VELNOR_MACOS_ARTIFACT_NAME" "$VELNOR_MACOS_BUNDLE_NAME"
    "$VELNOR_MANIFEST_ARTIFACT_NAME" "$VELNOR_MANIFEST_BUNDLE_NAME"
  )
  local -a digests=(
    "$VELNOR_LINUX_ARTIFACT_DIGEST" "$VELNOR_LINUX_BUNDLE_DIGEST"
    "$VELNOR_MACOS_ARTIFACT_DIGEST" "$VELNOR_MACOS_BUNDLE_DIGEST"
    "$VELNOR_MANIFEST_ARTIFACT_DIGEST" "$VELNOR_MANIFEST_BUNDLE_DIGEST"
  )
  for index in "${!ids[@]}"; do
    assets_json="$(jq -cn --argjson previous "$assets_json" \
      --argjson id "${ids[$index]}" --arg name "${names[$index]}" \
      --arg digest "sha256:${digests[$index]}" --argjson run_id 9001 \
      --argjson repository_id 1234 --arg sha "$FAKE_SHA" --arg branch main \
      '$previous + [{id:$id,name:$name,digest:$digest,size_in_bytes:100,expired:false,workflow_run:{id:$run_id,repository_id:$repository_id,head_repository_id:$repository_id,head_sha:$sha,head_branch:$branch}}]')"
  done
  export FAKE_ARTIFACTS_JSON="$state/artifacts.json"
  jq -cn --argjson artifacts "$assets_json" '{total_count:6,artifacts:$artifacts}' \
    > "$FAKE_ARTIFACTS_JSON"
}

write_release_fixture() {
  local state="$1" directory="$2" assets_json='[]' path digest size
  for path in "$directory"/assets/*; do
    path="${path##*/}"
    digest="$(sha256sum -- "$directory/assets/$path" | awk '{print $1}')"
    size="$(wc -c < "$directory/assets/$path" | tr -d ' ')"
    assets_json="$(jq -cn --argjson previous "$assets_json" --arg name "$path" \
      --arg digest "sha256:$digest" --argjson size "$size" \
      '$previous + [{name:$name,digest:$digest,size:$size}]')"
  done
  export FAKE_RELEASE_JSON="$state/release.json"
  jq -cn --arg tag 'v0.1.1' --argjson assets "$assets_json" \
    '{tag_name:$tag,draft:false,prerelease:false,assets:$assets}' > "$FAKE_RELEASE_JSON"
}

setup_publisher_case() {
  local name="$1" directory state
  local linux_binary macos_binary
  directory="$temp_root/$name"
  state="$directory/state"
  case_directory="$directory"
  case_state="$state"
  copy_checkout "$directory"
  mkdir -p "$directory/linux-assets" "$directory/macos-assets" "$directory/assets" "$state"
  linux_binary='velnor-actions-0.1.1-x86_64-unknown-linux-gnu'
  macos_binary='velnor-actions-0.1.1-aarch64-apple-darwin'
  printf 'fixture linux executable bytes\n' > "$directory/linux-assets/$linux_binary"
  printf 'fixture macos executable bytes\n' > "$directory/macos-assets/$macos_binary"
  write_sidecar "$directory/linux-assets" "$linux_binary"
  write_sidecar "$directory/macos-assets" "$macos_binary"
  (
    cd "$directory"
    env GITHUB_REPOSITORY='tailrocks/velnor-new' \
      GITHUB_SHA='0123456789abcdef0123456789abcdef01234567' \
      VELNOR_RELEASE_VERSION='0.1.1' VELNOR_RELEASE_TAG='v0.1.1' \
      bash scripts/generator-release/create-manifest.sh
  )
  cp "$directory/linux-assets/$linux_binary" "$directory/assets/"
  cp "$directory/linux-assets/$linux_binary.sha256" "$directory/assets/"
  cp "$directory/macos-assets/$macos_binary" "$directory/assets/"
  cp "$directory/macos-assets/$macos_binary.sha256" "$directory/assets/"
  cp "$directory/manifest-assets/velnor-actions-release-manifest.json" "$directory/assets/"
  for manifest in \
    'velnor-actions-0.1.1-linux-provenance.sigstore.json' \
    'velnor-actions-0.1.1-macos-provenance.sigstore.json' \
    'velnor-actions-0.1.1-manifest-provenance.sigstore.json'; do
    printf '{"mediaType":"application/vnd.dev.sigstore.bundle+json;version=0.2","verificationMaterial":{},"dsseEnvelope":{}}\n' \
      > "$directory/assets/$manifest"
  done

  export FAKE_STATE="$state" FAKE_SHA='0123456789abcdef0123456789abcdef01234567' FAKE_BRANCH='main'
  export GITHUB_REPOSITORY='tailrocks/velnor-new' GITHUB_REPOSITORY_ID='1234'
  export GITHUB_SHA="$FAKE_SHA" GITHUB_WORKFLOW_SHA="$FAKE_SHA"
  export GITHUB_EVENT_NAME='workflow_dispatch' GITHUB_REF='refs/heads/main'
  export GITHUB_WORKFLOW_REF='tailrocks/velnor-new/.github/workflows/generator-release.yml@refs/heads/main'
  export GITHUB_RUN_ID='9001' GITHUB_RUN_ATTEMPT='1' GITHUB_OUTPUT="$state/output"
  export GH_TOKEN='fixture-token' VELNOR_RELEASE_VERSION='0.1.1' VELNOR_RELEASE_TAG='v0.1.1'
  export VELNOR_BUILD_RUN_ID='9001' VELNOR_BUILD_RUN_ATTEMPT='1' VELNOR_REPOSITORY_ID='1234'
  export VELNOR_EXPECTED_DEFAULT_BRANCH='main' VELNOR_EXPECTED_WORKFLOW_SHA="$FAKE_SHA"
  export VELNOR_CI_POLL_LIMIT='1' VELNOR_RELEASE_CI_POLL_LIMIT='1' VELNOR_RELEASE_CI_POLL_SECONDS='0'
  export VELNOR_LINUX_ARTIFACT_ID='501' VELNOR_LINUX_ARTIFACT_DIGEST="$(printf 'a%.0s' {1..64})"
  export VELNOR_LINUX_BUNDLE_ID='502' VELNOR_LINUX_BUNDLE_DIGEST="$(printf 'b%.0s' {1..64})"
  export VELNOR_MACOS_ARTIFACT_ID='503' VELNOR_MACOS_ARTIFACT_DIGEST="$(printf 'c%.0s' {1..64})"
  export VELNOR_MACOS_BUNDLE_ID='504' VELNOR_MACOS_BUNDLE_DIGEST="$(printf 'd%.0s' {1..64})"
  export VELNOR_MANIFEST_ARTIFACT_ID='505' VELNOR_MANIFEST_ARTIFACT_DIGEST="$(printf 'e%.0s' {1..64})"
  export VELNOR_MANIFEST_BUNDLE_ID='506' VELNOR_MANIFEST_BUNDLE_DIGEST="$(printf 'f%.0s' {1..64})"
  export VELNOR_LINUX_ARTIFACT_NAME='generator-0.1.1-linux-assets-9001-1'
  export VELNOR_LINUX_BUNDLE_NAME='generator-0.1.1-linux-provenance-9001-1'
  export VELNOR_MACOS_ARTIFACT_NAME='generator-0.1.1-macos-assets-9001-1'
  export VELNOR_MACOS_BUNDLE_NAME='generator-0.1.1-macos-provenance-9001-1'
  export VELNOR_MANIFEST_ARTIFACT_NAME='generator-0.1.1-manifest-assets-9001-1'
  export VELNOR_MANIFEST_BUNDLE_NAME='generator-0.1.1-manifest-provenance-9001-1'
  export FAKE_CURL_MODE='ok' FAKE_GH_MODE='ok'
  write_artifact_fixture "$state"
  write_release_fixture "$state" "$directory"
}

run_publisher() {
  local directory="$1"
  (cd "$directory" && bash scripts/generator-release/publish.sh)
}

expect_publisher_failure() {
  local name="$1" gh_mode="${2:-ok}" curl_mode="${3:-ok}"
  local directory state
  setup_publisher_case "$name"
  directory="$case_directory"
  state="$case_state"
  export FAKE_GH_MODE="$gh_mode" FAKE_CURL_MODE="$curl_mode"
  if run_publisher "$directory" > "$state/log" 2>&1; then
    fail "$name unexpectedly succeeded"
  fi
  [[ ! -s "$state/tag-posts" && ! -s "$state/release-posts" ]] \
    || fail "$name performed a release mutation before validation completed"
}

source_preflight_tests() {
  local name directory state
  setup_publisher_case source_ok
  directory="$case_directory"
  state="$case_state"
  (cd "$directory" && bash scripts/generator-release/check-source.sh)
  grep -qx "source_sha=$FAKE_SHA" "$GITHUB_OUTPUT" || fail 'source SHA output is missing'
  grep -qx 'default_branch=main' "$GITHUB_OUTPUT" || fail 'default branch output is missing'

  for name in source_403 source_timeout source_malformed source_wrong_sha source_pending source_no_run; do
    setup_publisher_case "$name"
    directory="$case_directory"
    state="$case_state"
    export FAKE_GH_MODE="$name" VELNOR_RELEASE_CI_POLL_LIMIT='1'
    if (cd "$directory" && bash scripts/generator-release/check-source.sh) > "$state/log" 2>&1; then
      fail "$name source preflight unexpectedly succeeded"
    fi
  done
  setup_publisher_case source_wrong_ref
  directory="$case_directory"
  state="$case_state"
  export FAKE_GH_MODE='ok' GITHUB_REF='refs/heads/release'
  if (cd "$directory" && bash scripts/generator-release/check-source.sh) > "$state/log" 2>&1; then
    fail 'wrong source ref unexpectedly passed preflight'
  fi
}

publisher_rejections() {
  expect_publisher_failure publisher_403 ok publisher_403
  expect_publisher_failure publisher_timeout ok publisher_timeout
  expect_publisher_failure publisher_malformed ok publisher_malformed
  expect_publisher_failure publisher_artifact_substitution ok artifact_substitution
  expect_publisher_failure publisher_existing_tag ok existing_tag
  expect_publisher_failure publisher_existing_release ok existing_release

  local directory state manifest
  setup_publisher_case publisher_target_mismatch
  directory="$case_directory"
  state="$case_state"
  manifest="$directory/assets/velnor-actions-release-manifest.json"
  jq '.targets[0].target = "aarch64-apple-darwin"' "$manifest" > "$manifest.tmp"
  mv "$manifest.tmp" "$manifest"
  if run_publisher "$directory" > "$state/log" 2>&1; then
    fail 'wrong target in manifest unexpectedly passed validation'
  fi
  [[ ! -s "$state/tag-posts" ]] || fail 'target mismatch created a tag'

  setup_publisher_case publisher_attestation_mismatch
  directory="$case_directory"
  state="$case_state"
  export FAKE_GH_MODE='wrong_attestation' FAKE_CURL_MODE='ok'
  if run_publisher "$directory" > "$state/log" 2>&1; then
    fail 'wrong attestation source unexpectedly passed verification'
  fi
  [[ ! -s "$state/tag-posts" ]] || fail 'attestation mismatch created a tag'
}

lost_create_tests() {
  local directory state
  setup_publisher_case lost_tag_response
  directory="$case_directory"
  state="$case_state"
  export FAKE_GH_MODE='tag_lost'
  if run_publisher "$directory" > "$state/log" 2>&1; then
    fail 'lost tag-create response unexpectedly succeeded'
  fi
  [[ "$(cat "$state/tag-posts" 2>/dev/null || true)" == '1' ]] \
    || { cat "$state/log" >&2; fail 'publisher did not make exactly one uncertain tag creation request'; }
  export FAKE_GH_MODE='ok'
  if run_publisher "$directory" > "$state/retry.log" 2>&1; then
    fail 'publisher retried a version after uncertain tag creation'
  fi
  [[ "$(cat "$state/tag-posts")" == '1' ]] || fail 'tag creation retry occurred'

  setup_publisher_case lost_release_response
  directory="$case_directory"
  state="$case_state"
  export FAKE_GH_MODE='release_lost'
  if run_publisher "$directory" > "$state/log" 2>&1; then
    fail 'lost release-create response unexpectedly succeeded'
  fi
  [[ "$(cat "$state/tag-posts")" == '1' && "$(cat "$state/release-posts")" == '1' ]] \
    || fail 'uncertain release create was not a single attempt'
  export FAKE_GH_MODE='ok'
  if run_publisher "$directory" > "$state/retry.log" 2>&1; then
    fail 'publisher retried a version after uncertain release creation'
  fi
  [[ "$(cat "$state/tag-posts")" == '1' && "$(cat "$state/release-posts")" == '1' ]] \
    || fail 'release creation retry occurred'
}

success_test() {
  local directory state
  setup_publisher_case publisher_success
  directory="$case_directory"
  state="$case_state"
  export FAKE_GH_MODE='ok' FAKE_CURL_MODE='ok'
  run_publisher "$directory" > "$state/log" 2>&1 \
    || { cat "$state/log" >&2; fail 'valid immutable release did not complete through fake GitHub'; }
  [[ "$(cat "$state/tag-posts")" == '1' && "$(cat "$state/release-posts")" == '1' ]] \
    || fail 'valid release did not make one tag and one release request'
}

fixture_manifest_test
source_preflight_tests
publisher_rejections
lost_create_tests
success_test
printf 'generator release fake API/GH tests passed\n'
