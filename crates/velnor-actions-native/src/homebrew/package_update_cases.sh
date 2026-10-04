
reject_preview() {
  if run_updater "$1" preview "$2"; then
    echo "package updater accepted invalid fixture: $3" >&2
    exit 1
  fi
  unchanged_preview
}
variant() {
  local destination="$tmp/$1"
  cp -R "$preview" "$destination"
  printf '%s\n' "$destination"
}
for mode in both-fail malformed multiple-line; do
  case_id "preview-git-$mode"
  invalid=$(variant "git-$mode")
  : > "$VELNOR_FIXTURE_GIT_LOG"
  export VELNOR_FIXTURE_GIT_MODE="$mode"
  reject_preview "$invalid" preview "git-$mode"
  if test "$mode" = both-fail; then
    assert_queries 'refs/tags/preview^{}' refs/tags/preview
  else
    assert_queries 'refs/tags/preview^{}'
  fi
done
export VELNOR_FIXTURE_GIT_MODE=annotated-success
case_id preview-missing-tag
invalid=$(variant missing-tag)
reject_preview "$invalid" absent missing-tag
case_id preview-mismatched-tag
invalid=$(variant mismatched-tag)
reject_preview "$invalid" preview-foreign mismatched-tag
case_id preview-mismatched-ref
invalid=$(variant mismatched-ref)
export VELNOR_FIXTURE_GIT_COMMIT="$different_commit"
reject_preview "$invalid" preview mismatched-ref
export VELNOR_FIXTURE_GIT_COMMIT="$commit"
case_id preview-identity-disagreement
invalid=$(variant identity-disagreement)
jq --arg commit "$different_commit" '.manifest.source_commit = $commit' \
  "$invalid/identity.json" > "$tmp/changed.json"
mv "$tmp/changed.json" "$invalid/identity.json"
reject_preview "$invalid" preview identity-disagreement
case_id stable-missing-payload
invalid="$tmp/stable-missing-payload"
cp -R "$stable" "$invalid"
rm "$invalid/${stable_names[0]}"
if run_updater "$invalid" stable absent; then
  echo 'package updater accepted missing stable payload' >&2
  exit 1
fi
unchanged_stable
case_id preview-missing-supporting
invalid=$(variant missing-supporting)
rm "$invalid/SHA256SUMS"
reject_preview "$invalid" preview missing-supporting
case_id preview-extra-supporting
invalid=$(variant extra-supporting)
printf 'unexpected\n' > "$invalid/unlisted-supporting.asset"
reject_preview "$invalid" preview extra-supporting
case_id preview-tampered-supporting
invalid=$(variant tampered-supporting)
printf 'tampered\n' >> "$invalid/SHA256SUMS"
reject_preview "$invalid" preview tampered-supporting
case_id preview-reclassified-payload
invalid=$(variant reclassified-payload)
jq '.assets[-1] as $payload | .assets = .assets[0:-1] | .supporting_assets += [$payload]' \
  "$invalid/release-manifest.json" > "$tmp/changed.json"
mv "$tmp/changed.json" "$invalid/release-manifest.json"
jq --slurpfile manifest "$invalid/release-manifest.json" '.manifest = $manifest[0]' \
  "$invalid/identity.json" > "$tmp/changed.json"
mv "$tmp/changed.json" "$invalid/identity.json"
reject_preview "$invalid" preview reclassified-payload
printf '%s\n' 'velnor-package-fixture-complete: 16 cases'
