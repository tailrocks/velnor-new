
preview="$tmp/preview"
mkdir "$preview" "$tmp/binary"
export VELNOR_FIXTURE_BINARY_OUTPUT="$binary $preview_version"
printf '%s\n' '#!/usr/bin/env bash' 'printf "%s\n" "${VELNOR_FIXTURE_BINARY_OUTPUT:?}"' > "$tmp/binary/$binary"
chmod 0755 "$tmp/binary/$binary"
: > "$tmp/preview-assets.jsonl"
while IFS=$'\t' read -r name executable; do
  if test "$executable" = true; then
    tar -czf "$preview/$name" -C "$tmp/binary" -- "$binary"
  else
    printf 'fixture-%s\n' "$name" > "$preview/$name"
  fi
  asset_record "$preview" "$name" >> "$tmp/preview-assets.jsonl"
done < <(jq -r '.artifacts[] | select(.preview_name != null) | [.preview_name,.executable] | @tsv' <<< "$profile")
jq -s -r '.[] | "\(.sha256)  \(.name)"' "$tmp/preview-assets.jsonl" > "$preview/SHA256SUMS"
supporting_names=(SHA256SUMS)
for name in "${preview_names[@]}"; do
  supporting_names+=("$name.sha256" "$name.bundle" "$name.sbom.json")
done
while IFS= read -r name; do
  supporting_names+=("$name" "$name.bundle")
done < <(jq -r '.supporting_manifests[]' <<< "$profile")
: > "$tmp/supporting-assets.jsonl"
for name in "${supporting_names[@]}"; do
  case "$name" in
    SHA256SUMS) ;;
    *.sha256)
      payload=${name%.sha256}
      checksum=$(jq -s -er --arg name "$payload" '.[] | select(.name == $name) | .sha256' "$tmp/preview-assets.jsonl")
      printf '%s  %s\n' "$checksum" "$payload" > "$preview/$name"
      ;;
    *) printf 'support-fixture-%s\n' "$name" > "$preview/$name" ;;
  esac
  asset_record "$preview" "$name" >> "$tmp/supporting-assets.jsonl"
done
jq -Sn --arg source_repository "$repository" --arg source_ref refs/heads/main \
  --arg source_commit "$commit" --arg version "$preview_version" \
  --slurpfile assets "$tmp/preview-assets.jsonl" --slurpfile supporting_assets "$tmp/supporting-assets.jsonl" \
  '{schema:"velnor.package-release.v1",source_repository:$source_repository,source_ref:$source_ref,source_commit:$source_commit,version:$version,assets:$assets,supporting_assets:$supporting_assets}' > "$preview/release-manifest.json"
write_identity "$preview" refs/heads/main

mkdir "$tmp/git-bin"
cat > "$tmp/git-bin/git" <<'MOCK_GIT'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$#" -ne 4 || "$1" != ls-remote || "$2" != --exit-code ||
  "$3" != "https://github.com/${VELNOR_FIXTURE_REPOSITORY:?}.git" ||
  ("$4" != refs/tags/preview && "$4" != 'refs/tags/preview^{}') ]]; then
  echo 'unexpected git invocation' >&2
  exit 1
fi
printf '%s\n' "$4" >> "${VELNOR_FIXTURE_GIT_LOG:?}"
case "${VELNOR_FIXTURE_GIT_MODE:-annotated-success}" in
  annotated-success)
    test "$4" = 'refs/tags/preview^{}'
    printf '%s\t%s\n' "${VELNOR_FIXTURE_GIT_COMMIT:?}" "$4"
    ;;
  lightweight-fallback)
    if test "$4" = 'refs/tags/preview^{}'; then exit 2; fi
    printf '%s\t%s\n' "${VELNOR_FIXTURE_GIT_COMMIT:?}" "$4"
    ;;
  both-fail) exit 2 ;;
  malformed) printf 'not-a-commit\t%s\n' "$4" ;;
  multiple-line) printf '%s\t%s\nmalformed\n' "${VELNOR_FIXTURE_GIT_COMMIT:?}" "$4" ;;
  *) exit 1 ;;
esac
MOCK_GIT
chmod 0755 "$tmp/git-bin/git"
export PATH="$tmp/git-bin:$PATH"
export VELNOR_FIXTURE_REPOSITORY="$repository"
export VELNOR_FIXTURE_GIT_COMMIT="$commit"
export VELNOR_FIXTURE_GIT_LOG="$tmp/git.log"
export VELNOR_FIXTURE_GIT_MODE=annotated-success
: > "$VELNOR_FIXTURE_GIT_LOG"
assert_queries() {
  printf '%s\n' "$@" > "$tmp/expected-git.log"
  cmp -s "$tmp/expected-git.log" "$VELNOR_FIXTURE_GIT_LOG"
}
assert_preview_outputs() {
  local name checksum
  ruby_check "$tmp/repo/$preview_formula"
  grep -Fx "# source-sha: $commit" "$tmp/repo/$preview_formula" >/dev/null
  grep -F "version \"$preview_version\"" "$tmp/repo/$preview_formula" >/dev/null
  test "$(grep -cE 'sha256 "[0-9a-f]{64}"$' "$tmp/repo/$preview_formula")" -eq "${#preview_names[@]}"
  if grep -Eq 'sha256 "[0-9a-f]{64}  ' "$tmp/repo/$preview_formula"; then exit 1; fi
  for name in "${preview_names[@]}"; do
    checksum=$(jq -er --arg name "$name" '.assets[] | select(.name == $name) | .sha256' "$preview/release-manifest.json")
    output_pair "$preview_formula" "https://github.com/$repository/releases/download/preview/$name" "$checksum"
  done
}
case_id preview-annotated-success
run_updater "$preview" preview preview
assert_preview_outputs
snapshot_preview
assert_queries 'refs/tags/preview^{}'
case_id preview-lightweight-success
: > "$VELNOR_FIXTURE_GIT_LOG"
export VELNOR_FIXTURE_GIT_MODE=lightweight-fallback
run_updater "$preview" preview preview
assert_queries 'refs/tags/preview^{}' refs/tags/preview
unchanged_preview
export VELNOR_FIXTURE_GIT_MODE=annotated-success
