write_output_binding_candidate_wrapper() {
  local wrapper="$1"
  cat >"$wrapper" <<'WRAPPER'
#!/bin/sh
if [ "${1-}" = "--version" ]; then
  exec "$VELNOR_TEST_REAL_CLI_BIN" "$@"
fi
if [ "${1-}" != "generate" ]; then
  exec "$VELNOR_TEST_REAL_CLI_BIN" "$@"
fi
"$VELNOR_TEST_REAL_CLI_BIN" "$@" || exit $?
python3 - "$3/.github/workflows/ci.yml" "$VELNOR_TEST_OUTPUT_FAULT" \
  "$VELNOR_TEST_BAD_DIGEST" <<'PY'
from pathlib import Path
import json
import re
import sys

path = Path(sys.argv[1])
fault = sys.argv[2]
bad_digest = sys.argv[3]
lines = path.read_text(encoding="utf-8").splitlines()
field = re.compile(r"^(?P<indent>[ \t]*)(?P<key>VELNOR_ASSET_SHA256|VELNOR_ASSET_URL|VELNOR_RELEASE_COMMIT): (?P<value>\S+)$")
starts = [index for index, line in enumerate(lines)
          if field.fullmatch(line) and field.fullmatch(line).group("key") == "VELNOR_ASSET_SHA256"]
if len(starts) < 2:
    raise SystemExit("fault fixture needs at least two bindings to retain valid neighbors")
index = starts[0]
match = [field.fullmatch(line) for line in lines[index:index + 3]]
if len(match) != 3 or any(item is None for item in match):
    raise SystemExit("generated workflow did not contain a complete first binding")
if [item.group("key") for item in match] != [
    "VELNOR_ASSET_SHA256", "VELNOR_ASSET_URL", "VELNOR_RELEASE_COMMIT"
]:
    raise SystemExit("generated workflow fields were not adjacent in the expected order")

if fault in ("wrong-digest", "both"):
    lines[index] = match[0].group("indent") + "VELNOR_ASSET_SHA256: " + bad_digest
if fault in ("wrong-commit", "both"):
    lines[index + 2] = match[2].group("indent") + "VELNOR_RELEASE_COMMIT: " + ("b" * 40)
if fault == "missing-digest":
    del lines[index]
if fault == "missing-url":
    del lines[index + 1]
if fault == "missing-commit":
    del lines[index + 2]
if fault == "wrong-url":
    lines[index + 1] = match[1].group("indent") + "VELNOR_ASSET_URL: https://example.invalid/wrong-release-target"
if fault == "wrong-target":
    manifest = json.loads((Path.cwd() / ".velnor/release-manifest.json").read_text())
    current_digest = match[0].group("value")
    alternatives = [item for item in manifest["targets"]
                    if item["artifact"] != match[1].group("value")
                    and item["sha256"] != current_digest]
    if not alternatives:
        raise SystemExit("no distinct manifest target is available for the wrong-target case")
    lines[index + 1] = match[1].group("indent") + "VELNOR_ASSET_URL: " + alternatives[0]["artifact"]

path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY
WRAPPER
  chmod u+x "$wrapper"
}

prepare_output_binding_candidate() {
  local linux_sha arm_sha intel_sha host_target_name
  OUTPUT_BINDING_WRAPPER="$WORK/output-binding candidate"
  OUTPUT_BINDING_MANIFEST="$WORK/output-binding manifest.json"
  write_output_binding_candidate_wrapper "$OUTPUT_BINDING_WRAPPER" || return 2
  OUTPUT_BINDING_BINARY_SHA="$(test_file_sha256 "$OUTPUT_BINDING_WRAPPER")" || return 2
  linux_sha="$TEST_LINUX_SHA"
  arm_sha="$TEST_ARM_SHA"
  intel_sha="$TEST_INTEL_SHA"
  case "$HOST_TARGET_INDEX" in
    0) linux_sha="$OUTPUT_BINDING_BINARY_SHA"; host_target_name=x86_64-unknown-linux-gnu ;;
    1) arm_sha="$OUTPUT_BINDING_BINARY_SHA"; host_target_name=aarch64-apple-darwin ;;
    2) intel_sha="$OUTPUT_BINDING_BINARY_SHA"; host_target_name=x86_64-apple-darwin ;;
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
    || ! jq -e --arg target "$host_target_name" --arg digest "$OUTPUT_BINDING_BINARY_SHA" \
      --arg source "$SOURCE_SHA" '
      .commit == $source and
      ([.targets[] | select(.target == $target and .sha256 == $digest)] | length) == 1
    ' "$OUTPUT_BINDING_MANIFEST" >/dev/null; then
    echo "FAIL: output-binding test did not separate manifest and candidate SHA inputs" >&2
    return 1
  fi
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
    GITHUB_SHA="$SOURCE_SHA" \
    GITHUB_REPOSITORY=tailrocks/velnor-new \
    VELNOR_TEST_REAL_CLI_BIN="$CLI_BIN" \
    VELNOR_TEST_OUTPUT_FAULT="$fault" \
    VELNOR_TEST_BAD_DIGEST="$OUTPUT_BINDING_BAD_DIGEST" \
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
  retained="$(sed -n 's/^retaining failed golden workspace: //p' "$log")"
  if [[ -z "$retained" || "$retained" != "$WORK/tmp workspace"/velnor-goldens-work.* \
    || ! -d "$retained" ]]; then
    cat "$log" >&2
    echo "FAIL: candidate output binding $fault did not retain its failing workspace" >&2
    return 1
  fi
  rm -rf "$retained" || return 2
  echo "passed pre-normalization candidate binding rejection: $fault"
}

run_candidate_output_binding_tests() {
  local fault
  prepare_output_binding_candidate || return $?
  mkdir -p "$WORK/tmp workspace" || return 2
  for fault in wrong-digest wrong-commit both missing-digest missing-url missing-commit wrong-url wrong-target; do
    expect_output_binding_fault_rejected "$fault" || return $?
  done
}
