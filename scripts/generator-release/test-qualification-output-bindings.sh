# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

prepare_output_binding_candidate() {
  local linux_sha arm_sha intel_sha repo_root root_var=ROOT
  repo_root="${!root_var}"
  OUTPUT_BINDING_REPO="$WORK/output-binding repo"
  OUTPUT_BINDING_PREVIEW="$WORK/output-binding preview"
  OUTPUT_BINDING_MANIFEST="$WORK/output-binding manifest.json"
  OUTPUT_BINDING_DUPLICATE_MANIFEST="$WORK/output-binding duplicate manifest.json"
  OUTPUT_BINDING_BINARY_SHA="$CLI_SHA"
  linux_sha="$TEST_LINUX_SHA"
  arm_sha="$TEST_ARM_SHA"
  intel_sha="$TEST_INTEL_SHA"
  case "$HOST_TARGET_INDEX" in
    0) linux_sha="$OUTPUT_BINDING_BINARY_SHA" ;;
    1) arm_sha="$OUTPUT_BINDING_BINARY_SHA" ;;
    2) intel_sha="$OUTPUT_BINDING_BINARY_SHA" ;;
    *) echo "FAIL: unsupported host target index for binding test" >&2; return 1 ;;
  esac
  OUTPUT_BINDING_BAD_DIGEST="$(printf '%064d' 0 | tr '0' 'a')"
  if [ "$OUTPUT_BINDING_BAD_DIGEST" = "$linux_sha" ]; then
    OUTPUT_BINDING_BAD_DIGEST="$(printf '%064d' 0 | tr '0' 'd')"
  fi
  write_test_manifest "$OUTPUT_BINDING_MANIFEST" "$SOURCE_SHA" \
    "$linux_sha" "$arm_sha" "$intel_sha" || return 2
  OUTPUT_BINDING_MANIFEST_SHA="$(test_file_sha256 "$OUTPUT_BINDING_MANIFEST")" || return 2
  if [[ ! "$OUTPUT_BINDING_BINARY_SHA" =~ ^[0-9a-f]{64}$ \
    || ! "$OUTPUT_BINDING_MANIFEST_SHA" =~ ^[0-9a-f]{64}$ \
    || "$OUTPUT_BINDING_MANIFEST_SHA" = "$OUTPUT_BINDING_BINARY_SHA" ]] \
    || ! jq -e --arg target "$HOST_TARGET" --arg digest "$OUTPUT_BINDING_BINARY_SHA" \
      --arg source "$SOURCE_SHA" '
      .commit == $source and
      ([.targets[] | select(.target == $target and .sha256 == $digest)] | length) == 1
    ' "$OUTPUT_BINDING_MANIFEST" >/dev/null; then
    echo "FAIL: output-binding manifest does not bind the real CLI digest" >&2
    return 1
  fi
  mkdir -p "$OUTPUT_BINDING_REPO/.velnor" || return 2
  cp -RP "$repo_root/fixtures/empty-suite/." "$OUTPUT_BINDING_REPO/" || return 2
  cp "$OUTPUT_BINDING_MANIFEST" "$OUTPUT_BINDING_REPO/.velnor/release-manifest.json" || return 2
  fixture_git -C "$OUTPUT_BINDING_REPO" init -q || return 2
  fixture_git -C "$OUTPUT_BINDING_REPO" remote add origin https://github.com/tailrocks/velnor-new.git || return 2
  fixture_git -C "$OUTPUT_BINDING_REPO" add -A || return 2
  fixture_git -C "$OUTPUT_BINDING_REPO" commit -qm "output binding fixture" || return 2
  verify_qualification_guard || return 2
  if ! (
    for key in ${!GIT_@}; do unset "$key"; done
    export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_GLOBAL=/dev/null
    export GIT_TEMPLATE_DIR=""
    cd "$OUTPUT_BINDING_REPO" \
    && GITHUB_SHA="$SOURCE_SHA" GITHUB_REPOSITORY=tailrocks/velnor-new \
      run_qualification_candidate "$CLI_BIN" generate --output-dir "$OUTPUT_BINDING_PREVIEW"
  ); then
    echo "FAIL: actual candidate CLI did not generate the output-binding preview" >&2
    return 1
  fi
  if [ ! -d "$OUTPUT_BINDING_PREVIEW/.github" ]; then
    echo "FAIL: actual candidate CLI generated no workflow preview" >&2
    return 1
  fi
  if ! validate_output_bindings_for_manifest "$OUTPUT_BINDING_MANIFEST" "$OUTPUT_BINDING_PREVIEW"; then
    echo "FAIL: untouched real CLI preview did not pass output-binding validation" >&2
    return 1
  fi
  OUTPUT_BINDING_PREVIEW_SHA="$(output_binding_preview_fingerprint "$OUTPUT_BINDING_PREVIEW")" || return 2
  echo "candidate binary SHA=$OUTPUT_BINDING_BINARY_SHA; candidate manifest SHA=$OUTPUT_BINDING_MANIFEST_SHA"
}

validate_output_bindings_for_manifest() {
  local CANDIDATE_MANIFEST="$1"
  validate_candidate_output_bindings "$2"
}

expected_output_binding_diagnostic() {
  case "$1" in
    wrong-digest|both|wrong-target) echo "wrong digest for target $OUTPUT_BINDING_EXPECTED_TARGET" ;;
    wrong-commit) echo "wrong source commit" ;;
    missing-digest|missing-url|missing-commit) echo "incomplete binding" ;;
    wrong-url) echo "unknown release URL" ;;
    *) return 1 ;;
  esac
}

output_binding_preview_fingerprint() {
  local root="$1"
  (
    cd "$root" || exit 2
    {
      find . -type f -print | LC_ALL=C sort | while IFS= read -r path; do
        digest="$(test_file_sha256 "$path")" || exit 2
        printf '%s %s\n' "$digest" "$path"
      done
      find . -type l -print | LC_ALL=C sort | while IFS= read -r path; do
        printf 'link %s -> %s\n' "$path" "$(readlink "$path")"
      done
    } | test_stdin_sha256
  )
}

mutate_output_binding_preview() {
  local fault="$1" preview="$2" workflow wrong_url
  local candidate
  workflow=""
  while IFS= read -r candidate; do
    if awk '
      {
        line=$0
        sub(/^[ \t]+/, "", line)
        if (state == 0 && line ~ /^VELNOR_ASSET_SHA256: [^[:space:]]+$/) { state=1; next }
        if (state == 1 && line ~ /^VELNOR_ASSET_URL: [^[:space:]]+$/) { state=2; next }
        if (state == 2 && line ~ /^VELNOR_RELEASE_COMMIT: [^[:space:]]+$/) { found=1; exit }
        state=0
      }
      END { exit !found }
    ' "$candidate"; then
      workflow="$candidate"
      break
    fi
  done < <(find "$preview/.github" -type f -print | LC_ALL=C sort)
  if [ -z "$workflow" ]; then
    echo "FAIL: generated preview has no output-binding workflow" >&2
    return 1
  fi
  wrong_url=""
  local first_url target_record actual_target actual_digest
  first_url="$(awk '
    {
      line=$0
      sub(/^[ \t]+/, "", line)
      if (state == 0 && line ~ /^VELNOR_ASSET_SHA256: [^[:space:]]+$/) { state=1; next }
      if (state == 1 && line ~ /^VELNOR_ASSET_URL: [^[:space:]]+$/) {
        sub(/^VELNOR_ASSET_URL: /, "", line)
        url=line
        state=2
        next
      }
      if (state == 2 && line ~ /^VELNOR_RELEASE_COMMIT: [^[:space:]]+$/) { print url; exit }
      state=0
    }
  ' "$workflow")" || return 1
  target_record="$(jq -cer --arg url "$first_url" \
    '[.targets[] | select(.artifact == $url)] | if length == 1 then .[0] else error("binding URL has no unique manifest target") end' \
    "$OUTPUT_BINDING_MANIFEST")" || return 1
  actual_target="$(jq -er '.target' <<<"$target_record")" || return 1
  actual_digest="$(jq -er '.sha256' <<<"$target_record")" || return 1
  OUTPUT_BINDING_EXPECTED_TARGET="$actual_target"
  if [ "$fault" = wrong-target ]; then
    wrong_url="$(jq -er --arg target "$actual_target" --arg url "$first_url" --arg digest "$actual_digest" \
      '[.targets[] | select(.target != $target and .artifact != $url and .sha256 != $digest)] | .[0].artifact // error("no target has a distinct URL and digest")' \
      "$OUTPUT_BINDING_MANIFEST")" || return 1
    OUTPUT_BINDING_EXPECTED_TARGET="$(jq -er --arg url "$wrong_url" '[.targets[] | select(.artifact == $url)] | if length == 1 then .[0].target else error("alternate target is not unique") end' "$OUTPUT_BINDING_MANIFEST")" || return 1
    if [ "$wrong_url" = "$first_url" ] \
      || [ "$(jq -er --arg url "$wrong_url" '.targets[] | select(.artifact == $url) | .sha256' "$OUTPUT_BINDING_MANIFEST")" = "$actual_digest" ]; then
      echo "FAIL: alternate manifest target does not have a distinct URL and digest" >&2
      return 1
    fi
  fi
  awk -v fault="$fault" -v bad_digest="$OUTPUT_BINDING_BAD_DIGEST" \
    -v wrong_url="$wrong_url" -v wrong_commit="$OUTPUT_BINDING_BAD_COMMIT" '
    function key_value(line, expected,    indent, body, separator, key, value) {
      if (line !~ /^[ \t]*VELNOR_ASSET_SHA256: [^[:space:]]+$/ &&
          line !~ /^[ \t]*VELNOR_ASSET_URL: [^[:space:]]+$/ &&
          line !~ /^[ \t]*VELNOR_RELEASE_COMMIT: [^[:space:]]+$/) return 0
      indent=line
      sub(/[^ \t].*$/, "", indent)
      body=substr(line, length(indent) + 1)
      separator=index(body, ": ")
      key=substr(body, 1, separator - 1)
      value=substr(body, separator + 2)
      if (key != expected) return 0
      saved_indent[expected]=indent
      saved_value[expected]=value
      return 1
    }
    { lines[NR]=$0 }
    END {
      for (i=1; i+2<=NR; i++) {
        delete saved_indent
        delete saved_value
        if (key_value(lines[i], "VELNOR_ASSET_SHA256") &&
            key_value(lines[i+1], "VELNOR_ASSET_URL") &&
            key_value(lines[i+2], "VELNOR_RELEASE_COMMIT")) {
          start=i
          break
        }
      }
      if (start == 0) {
        print "generated workflow fields were not adjacent in the expected order" > "/dev/stderr"
        exit 1
      }
      if (fault == "wrong-digest" || fault == "both")
        lines[start]=saved_indent["VELNOR_ASSET_SHA256"] "VELNOR_ASSET_SHA256: " bad_digest
      if (fault == "wrong-commit" || fault == "both")
        lines[start+2]=saved_indent["VELNOR_RELEASE_COMMIT"] "VELNOR_RELEASE_COMMIT: " wrong_commit
      if (fault == "missing-digest") lines[start]=saved_indent["VELNOR_ASSET_SHA256"] "VELNOR_ASSET_SHA256: "
      if (fault == "missing-url") lines[start+1]=saved_indent["VELNOR_ASSET_URL"] "VELNOR_ASSET_URL: "
      if (fault == "missing-commit") lines[start+2]=saved_indent["VELNOR_RELEASE_COMMIT"] "VELNOR_RELEASE_COMMIT: "
      if (fault == "wrong-url")
        lines[start+1]=saved_indent["VELNOR_ASSET_URL"] "VELNOR_ASSET_URL: https://example.invalid/wrong-release-target"
      if (fault == "wrong-target")
        lines[start+1]=saved_indent["VELNOR_ASSET_URL"] "VELNOR_ASSET_URL: " wrong_url
      for (i=1; i<=NR; i++) print lines[i]
    }
  ' "$workflow" >"$workflow.mutated" || return 1
  mv "$workflow.mutated" "$workflow" || return 1
}

expect_output_binding_fault_rejected() {
  local fault="$1" expected status=0
  local mutation="$WORK/output-binding-$fault" copied_before before after validation_output original_after
  mkdir -p "$mutation" || return 2
  cp -RP "$OUTPUT_BINDING_PREVIEW/." "$mutation/" || return 2
  copied_before="$(output_binding_preview_fingerprint "$mutation")" || return 2
  OUTPUT_BINDING_BAD_COMMIT="$(printf '%040d' 0 | tr '0' 'b')"
  if [ "$OUTPUT_BINDING_BAD_COMMIT" = "$SOURCE_SHA" ]; then
    OUTPUT_BINDING_BAD_COMMIT="$(printf '%040d' 0 | tr '0' 'c')"
  fi
  mutate_output_binding_preview "$fault" "$mutation" || return 2
  expected="$(expected_output_binding_diagnostic "$fault")" || return 2
  before="$(output_binding_preview_fingerprint "$mutation")" || return 2
  if [ "$copied_before" = "$before" ]; then
    echo "FAIL: fault mutation $fault did not change its preview copy" >&2
    return 1
  fi
  verify_qualification_guard || return 2
  validation_output="$(validate_output_bindings_for_manifest \
    "$OUTPUT_BINDING_MANIFEST" "$mutation" 2>&1)" || status=$?
  verify_qualification_guard || return 2
  after="$(output_binding_preview_fingerprint "$mutation")" || return 2
  original_after="$(output_binding_preview_fingerprint "$OUTPUT_BINDING_PREVIEW")" || return 2
  if [ "$status" -eq 0 ] || ! grep -Fq "candidate output binding mismatch: $expected" <<<"$validation_output"; then
    printf '%s\n' "$validation_output" >&2
    echo "FAIL: real output-binding validator accepted fault $fault (exit $status)" >&2
    return 1
  fi
  if [ "$before" != "$after" ]; then
    echo "FAIL: output-binding validator changed rejected preview for $fault" >&2
    return 1
  fi
  if [ "$original_after" != "$OUTPUT_BINDING_PREVIEW_SHA" ]; then
    echo "FAIL: fault test $fault changed the untouched real CLI preview" >&2
    return 1
  fi
  echo "passed real output-binding rejection: $fault"
}

expect_normalized_output_bindings_valid() {
  local normalized_preview="$WORK/output-binding normalized preview"
  local normalized_manifest="$WORK/output-binding normalized manifest.json"
  local original_manifest="${CANDIDATE_MANIFEST-}" manifest_was_set=0 digest_placeholder commit_placeholder
  if [[ ${CANDIDATE_MANIFEST+x} ]]; then manifest_was_set=1; fi
  mkdir -p "$normalized_preview" || return 2
  cp -RP "$OUTPUT_BINDING_PREVIEW/." "$normalized_preview/" || return 2
  cp "$OUTPUT_BINDING_MANIFEST" "$normalized_manifest" || return 2
  digest_placeholder="$(printf '%064d' 0 | tr '0' 'a')"
  commit_placeholder="$(printf '%040d' 0 | tr '0' 'b')"
  CANDIDATE_MANIFEST="$OUTPUT_BINDING_MANIFEST"
  if ! normalize_candidate_digests "$normalized_preview"; then
    if [ "$manifest_was_set" -eq 1 ]; then CANDIDATE_MANIFEST="$original_manifest"; else unset CANDIDATE_MANIFEST; fi
    return 1
  fi
  if ! jq --arg digest "$digest_placeholder" --arg commit "$commit_placeholder" \
    '.commit = $commit | .targets[].sha256 = $digest' \
    "$normalized_manifest" >"$normalized_manifest.tmp" \
    || ! mv "$normalized_manifest.tmp" "$normalized_manifest"; then
    if [ "$manifest_was_set" -eq 1 ]; then CANDIDATE_MANIFEST="$original_manifest"; else unset CANDIDATE_MANIFEST; fi
    return 2
  fi
  if [ "$manifest_was_set" -eq 1 ]; then CANDIDATE_MANIFEST="$original_manifest"; else unset CANDIDATE_MANIFEST; fi
  if ! validate_output_bindings_for_manifest "$normalized_manifest" "$normalized_preview"; then
    echo "FAIL: normalized valid output bindings did not validate" >&2
    return 1
  fi
  if ! validate_output_bindings_for_manifest "$OUTPUT_BINDING_MANIFEST" "$OUTPUT_BINDING_PREVIEW"; then
    echo "FAIL: original valid output bindings changed during normalization" >&2
    return 1
  fi
  echo "passed normalized valid output-binding copy"
}

expect_duplicate_manifest_keys_rejected() {
  local status=0 log="$WORK/duplicate-manifest-keys.log" after baseline_fingerprint baseline_var=BEFORE
  baseline_fingerprint="${!baseline_var}"
  GITHUB_SHA="$SOURCE_SHA" GITHUB_REPOSITORY=tailrocks/velnor-new \
    PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    "$SCRIPT" check-release "$CLI_BIN" \
      "$OUTPUT_BINDING_DUPLICATE_MANIFEST" \
      "$(test_file_sha256 "$OUTPUT_BINDING_DUPLICATE_MANIFEST")" >"$log" 2>&1 || status=$?
  if [ "$status" -ne 2 ] \
    || ! grep -Fq 'duplicate object key' "$log" \
    || ! grep -Fq 'FATAL: candidate manifest contains duplicate JSON keys' "$log"; then
    cat "$log" >&2
    echo "FAIL: duplicate candidate manifest keys were not rejected by trusted-source checker (exit $status)" >&2
    return 1
  fi
  if [ -e "$MARKER" ]; then
    echo "FAIL: duplicate candidate manifest check invoked Cargo build" >&2
    return 1
  fi
  after="$(golden_fingerprint)" || return 2
  if [ "$after" != "$baseline_fingerprint" ]; then
    echo "FAIL: duplicate candidate manifest rejection changed goldens" >&2
    return 1
  fi
  echo "passed independent duplicate candidate manifest rejection"
}

run_candidate_output_binding_tests() {
  local fault
  prepare_output_binding_candidate || return $?
  for fault in wrong-digest wrong-commit both missing-digest missing-url missing-commit wrong-url wrong-target; do
    expect_output_binding_fault_rejected "$fault" || return $?
  done
  expect_normalized_output_bindings_valid || return $?
  sed 's/"schema": 1/"schema": 1, "schema": 1/' \
    "$OUTPUT_BINDING_MANIFEST" >"$OUTPUT_BINDING_DUPLICATE_MANIFEST" || return 2
  expect_duplicate_manifest_keys_rejected || return $?
}
