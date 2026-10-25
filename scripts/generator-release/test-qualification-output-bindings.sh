write_output_binding_candidate_wrapper() {
  local wrapper="$1"
  cat >"$wrapper" <<'WRAPPER'
#!/bin/sh
if [ "${VELNOR_INTERNAL_OP-}" = "repo-policy-v1" ]; then
  if [ -n "${VELNOR_TEST_REPO_POLICY_MARKER-}" ]; then
    printf '%s\n' "${VELNOR_REPO_POLICY_ACTION-unset}" >>"$VELNOR_TEST_REPO_POLICY_MARKER"
  fi
  exit 0
fi
if [ "${1-}" = "--version" ]; then
  exec "$VELNOR_TEST_REAL_CLI_BIN" "$@"
fi
if [ "${1-}" != "generate" ]; then
  exec "$VELNOR_TEST_REAL_CLI_BIN" "$@"
fi
"$VELNOR_TEST_REAL_CLI_BIN" "$@" || exit $?
workflow="$3/.github/workflows/ci.yml"
wrong_url=''
if [ "$VELNOR_TEST_OUTPUT_FAULT" = wrong-target ]; then
  first_binding="$(awk '
    /^[ \t]*VELNOR_ASSET_SHA256: [^[:space:]]+$/ && digest == "" { digest=$2 }
    /^[ \t]*VELNOR_ASSET_URL: [^[:space:]]+$/ && url == "" { url=$2 }
    END { if (digest != "" && url != "") print digest "|" url }
  ' "$workflow")" || exit 1
  current_digest=${first_binding%%|*}
  current_url=${first_binding#*|}
  wrong_url="$(jq -er --arg current_url "$current_url" --arg current_digest "$current_digest" '
    [.targets[] | select(.artifact != $current_url and .sha256 != $current_digest)][0].artifact
      // error("no distinct manifest target is available for the wrong-target case")
  ' .velnor/release-manifest.json)" || exit 1
fi
awk -v fault="$VELNOR_TEST_OUTPUT_FAULT" -v bad_digest="$VELNOR_TEST_BAD_DIGEST" \
  -v wrong_url="$wrong_url" '
  function field(line, expected,    body, separator, key, value, indent) {
    if (line !~ /^[ \t]*(VELNOR_ASSET_SHA256|VELNOR_ASSET_URL|VELNOR_RELEASE_COMMIT): [^[:space:]]+$/) return 0
    indent=line
    sub(/[^ \t].*$/, "", indent)
    body=substr(line, length(indent) + 1)
    separator=index(body, ": ")
    key=substr(body, 1, separator - 1)
    value=substr(body, separator + 2)
    if (key != expected) return 0
    keys[++found]=key
    indents[found]=indent
    values[found]=value
    return 1
  }
  { lines[NR]=$0 }
  END {
    for (i=1; i<=NR; i++) {
      if (lines[i] ~ /^[ \t]*VELNOR_ASSET_SHA256: [^[:space:]]+$/) starts[++count]=i
    }
    if (count < 2) { print "fault fixture needs at least two bindings to retain valid neighbors" > "/dev/stderr"; exit 1 }
    start=starts[1]
    found=0
    if (!field(lines[start], "VELNOR_ASSET_SHA256") ||
        !field(lines[start+1], "VELNOR_ASSET_URL") ||
        !field(lines[start+2], "VELNOR_RELEASE_COMMIT")) {
      print "generated workflow fields were not adjacent in the expected order" > "/dev/stderr"; exit 1
    }
    if (fault == "wrong-digest" || fault == "both") lines[start]=indents[1] "VELNOR_ASSET_SHA256: " bad_digest
    if (fault == "wrong-commit" || fault == "both") lines[start+2]=indents[3] "VELNOR_RELEASE_COMMIT: " sprintf("%040d", 0)
    if (fault == "wrong-commit" || fault == "both") sub(/0+$/, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", lines[start+2])
    if (fault == "missing-digest") skip[start]=1
    if (fault == "missing-url") skip[start+1]=1
    if (fault == "missing-commit") skip[start+2]=1
    if (fault == "wrong-url") lines[start+1]=indents[2] "VELNOR_ASSET_URL: https://example.invalid/wrong-release-target"
    if (fault == "wrong-target") lines[start+1]=indents[2] "VELNOR_ASSET_URL: " wrong_url
    for (i=1; i<=NR; i++) if (!skip[i]) print lines[i]
  }
' "$workflow" >"$workflow.mutated" || exit 1
mv "$workflow.mutated" "$workflow" || exit 1
WRAPPER
  chmod u+x "$wrapper"
}

prepare_output_binding_candidate() {
  local linux_sha arm_sha intel_sha
  OUTPUT_BINDING_WRAPPER="$WORK/output-binding candidate"
  OUTPUT_BINDING_MANIFEST="$WORK/output-binding manifest.json"
  OUTPUT_BINDING_DUPLICATE_MANIFEST="$WORK/output-binding duplicate manifest.json"
  OUTPUT_BINDING_REPO_POLICY_MARKER="$WORK/candidate repo-policy calls"
  write_output_binding_candidate_wrapper "$OUTPUT_BINDING_WRAPPER" || return 2
  OUTPUT_BINDING_BINARY_SHA="$(test_file_sha256 "$OUTPUT_BINDING_WRAPPER")" || return 2
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
    echo "FAIL: output-binding test did not separate manifest and candidate SHA inputs" >&2
    return 1
  fi
  if ! VELNOR_INTERNAL_OP=repo-policy-v1 \
    VELNOR_REPO_POLICY_ACTION=qualification-output-bindings \
    VELNOR_TEST_REAL_CLI_BIN="$CLI_BIN" \
    VELNOR_TEST_REPO_POLICY_MARKER="$OUTPUT_BINDING_REPO_POLICY_MARKER" \
    "$OUTPUT_BINDING_WRAPPER"; then
    echo "FAIL: candidate repo-policy lie fixture did not return success" >&2
    return 1
  fi
  if ! grep -Fq 'qualification-output-bindings' "$OUTPUT_BINDING_REPO_POLICY_MARKER"; then
    echo "FAIL: candidate repo-policy lie fixture did not record its false success" >&2
    return 1
  fi
  rm -f "$OUTPUT_BINDING_REPO_POLICY_MARKER"
  sed 's/"schema": 1/"schema": 1, "schema": 1/' \
    "$OUTPUT_BINDING_MANIFEST" >"$OUTPUT_BINDING_DUPLICATE_MANIFEST" || return 2
  echo "candidate binary SHA=$OUTPUT_BINDING_BINARY_SHA; candidate manifest SHA=$OUTPUT_BINDING_MANIFEST_SHA"
}

expected_output_binding_diagnostic() {
  case "$1" in
    wrong-digest|both) echo "wrong digest for target x86_64-unknown-linux-gnu" ;;
    wrong-commit) echo "wrong source commit" ;;
    missing-digest|missing-url|missing-commit) echo "incomplete binding" ;;
    wrong-url) echo "unknown release URL" ;;
    wrong-target) echo "wrong digest for target" ;;
    *) return 1 ;;
  esac
}

expect_output_binding_fault_rejected() {
  local fault="$1" expected status=0 log retained
  expected="$(expected_output_binding_diagnostic "$fault")" || return 2
  log="$WORK/output-binding-$fault.log"
  TMPDIR="$WORK/tmp workspace" \
    PATH="$STUB_BIN:$ORIGINAL_PATH" \
    VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    VELNOR_TEST_REAL_MISE_BIN="$REAL_MISE_BIN" \
    GITHUB_SHA="$SOURCE_SHA" \
    GITHUB_REPOSITORY=tailrocks/velnor-new \
    VELNOR_TEST_REAL_CLI_BIN="$CLI_BIN" \
    VELNOR_TEST_OUTPUT_FAULT="$fault" \
    VELNOR_TEST_BAD_DIGEST="$OUTPUT_BINDING_BAD_DIGEST" \
    VELNOR_TEST_REPO_POLICY_MARKER="$OUTPUT_BINDING_REPO_POLICY_MARKER" \
    "$SCRIPT" check-release "$OUTPUT_BINDING_WRAPPER" "$OUTPUT_BINDING_MANIFEST" \
      "$OUTPUT_BINDING_MANIFEST_SHA" >"$log" 2>&1 || status=$?
  if [ "$status" -ne 2 ] \
    || ! grep -Fq 'candidate output binding mismatch:' "$log" \
    || ! grep -Fq "$expected" "$log" \
    || ! grep -Fq 'FATAL: release candidate output bindings do not match candidate manifest' "$log"; then
    cat "$log" >&2
    echo "FAIL: malformed candidate output binding $fault was not rejected before normalization (exit $status)" >&2
    return 1
  fi
  if grep -Fq 'could not normalize candidate asset digests' "$log"; then
    cat "$log" >&2
    echo "FAIL: candidate output binding $fault reached normalization" >&2
    return 1
  fi
  if [ -e "$OUTPUT_BINDING_REPO_POLICY_MARKER" ]; then
    cat "$log" >&2
    echo "FAIL: release qualification asked candidate repo-policy to validate output bindings" >&2
    return 1
  fi
  retained="$(sed -n 's/^retaining failed golden workspace: //p' "$log")"
  if [[ -z "$retained" || "$retained" != "$WORK/tmp workspace"/velnor-goldens-work.* \
    || ! -d "$retained" ]]; then
    cat "$log" >&2
    echo "FAIL: candidate output binding $fault did not retain its failing workspace" >&2
    return 1
  fi
  rm -rf "$retained" || return 2
  echo "passed pre-normalization independent candidate binding rejection: $fault"
}

expect_duplicate_manifest_keys_rejected() {
  local status=0 log="$WORK/duplicate-manifest-keys.log" after
  GITHUB_SHA="$SOURCE_SHA" GITHUB_REPOSITORY=tailrocks/velnor-new \
    PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    VELNOR_TEST_REAL_CLI_BIN="$CLI_BIN" \
    VELNOR_TEST_REPO_POLICY_MARKER="$OUTPUT_BINDING_REPO_POLICY_MARKER" \
    "$SCRIPT" check-release "$OUTPUT_BINDING_WRAPPER" \
      "$OUTPUT_BINDING_DUPLICATE_MANIFEST" \
      "$(test_file_sha256 "$OUTPUT_BINDING_DUPLICATE_MANIFEST")" >"$log" 2>&1 || status=$?
  if [ "$status" -ne 2 ] \
    || ! grep -Fq 'duplicate object key' "$log" \
    || ! grep -Fq 'FATAL: candidate manifest contains duplicate JSON keys' "$log"; then
    cat "$log" >&2
    echo "FAIL: duplicate candidate manifest keys were not rejected by trusted-source checker (exit $status)" >&2
    return 1
  fi
  if [ -e "$OUTPUT_BINDING_REPO_POLICY_MARKER" ] || [ -e "$MARKER" ]; then
    echo "FAIL: duplicate candidate manifest check invoked candidate repo-policy or cargo build" >&2
    return 1
  fi
  after="$(golden_fingerprint)" || return 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: duplicate candidate manifest rejection changed goldens" >&2
    return 1
  fi
  echo "passed independent duplicate candidate manifest rejection"
}

run_candidate_output_binding_tests() {
  local fault
  prepare_output_binding_candidate || return $?
  mkdir -p "$WORK/tmp workspace" || return 2
  for fault in wrong-digest wrong-commit both missing-digest missing-url missing-commit wrong-url wrong-target; do
    expect_output_binding_fault_rejected "$fault" || return $?
  done
  expect_duplicate_manifest_keys_rejected || return $?
}
