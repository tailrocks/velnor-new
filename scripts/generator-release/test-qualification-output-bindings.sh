# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

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
fixture_manifest=.velnor/release-manifest.json
fixture_backup_dir=${VELNOR_TEST_MANIFEST_BACKUP_DIR-}
fixture_original=$fixture_backup_dir/original.json
fixture_replacement=$fixture_backup_dir/replacement.json
fixture_receipt=$fixture_backup_dir/receipt.json
fixture_install=$fixture_manifest.fixture.$$
fixture_backup_ready=0
if [ -z "$fixture_backup_dir" ] || [ -L "$fixture_backup_dir" ] || [ ! -d "$fixture_backup_dir" ] \
  || [ -L .velnor ] || [ ! -d .velnor ] || [ -L "$fixture_manifest" ] || [ ! -f "$fixture_manifest" ] \
  || [ -e "$fixture_original" ] || [ -e "$fixture_replacement" ] || [ -e "$fixture_receipt" ] \
  || [ -e "$fixture_install" ] || [ -L "$fixture_install" ]; then
  echo 'FAIL: disposable manifest fixture paths are missing or unsafe' >&2
  exit 1
fi
case "$fixture_backup_dir" in
  "$PWD"|"$PWD"/*)
    echo 'FAIL: disposable manifest evidence must remain outside the scratch repository' >&2
    exit 1
    ;;
esac
restore_fixture_manifest() {
  if [ "$fixture_backup_ready" -ne 1 ]; then
    return 0
  fi
  if [ -L .velnor ] || [ ! -d .velnor ]; then
    echo 'FATAL: scratch manifest parent is missing or unsafe during restoration' >&2
    return 1
  fi
  if [ -f "$fixture_original" ] && [ ! -L "$fixture_original" ] \
    && { [ ! -f "$fixture_manifest" ] || [ -L "$fixture_manifest" ] \
      || ! cmp -s "$fixture_original" "$fixture_manifest"; }; then
    restore_tmp=$fixture_manifest.restore.$$
    if ! cp -p "$fixture_original" "$restore_tmp" \
      || ! mv -f "$restore_tmp" "$fixture_manifest" \
      || ! cmp -s "$fixture_original" "$fixture_manifest"; then
      rm -f "$restore_tmp"
      echo 'FATAL: could not restore the exact scratch manifest bytes' >&2
      return 1
    fi
  fi
  return 0
}
cleanup_fixture_manifest() {
  cleanup_status=$?
  trap - EXIT HUP INT TERM
  if ! restore_fixture_manifest; then cleanup_status=1; fi
  rm -f "$fixture_install" "$fixture_manifest.restore.$$"
  exit "$cleanup_status"
}
trap cleanup_fixture_manifest EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
if ! cp -p "$fixture_manifest" "$fixture_original" \
  || ! cmp -s "$fixture_manifest" "$fixture_original"; then
  echo 'FATAL: could not save the exact scratch manifest bytes outside the fixture' >&2
  exit 1
fi
fixture_backup_ready=1
if ! python3 - "$fixture_original" "$fixture_replacement" "$fixture_receipt" \
  "$VELNOR_TEST_HOST_TARGET" "$VELNOR_TEST_REAL_CLI_SHA256" "$VELNOR_TEST_CANDIDATE_SHA256" "$PWD/$fixture_manifest" <<'PY'
import hashlib
import json
import re
from pathlib import Path
import sys

original_path, replacement_path, receipt_path = map(Path, sys.argv[1:4])
host_target, real_sha, candidate_sha, manifest_path = sys.argv[4:]
original = original_path.read_bytes()
if not re.fullmatch(r'[0-9a-f]{64}', real_sha) or not re.fullmatch(r'[0-9a-f]{64}', candidate_sha):
    raise SystemExit('fixture host digests are malformed')
if real_sha == candidate_sha:
    raise SystemExit('fixture requires distinct real and wrapper host digests')
document = json.loads(original)
targets = document.get('targets')
if not isinstance(targets, list):
    raise SystemExit('fixture manifest has no target list')
matches = [row for row in targets if isinstance(row, dict) and row.get('target') == host_target]
if len(matches) != 1 or matches[0].get('sha256') != candidate_sha:
    raise SystemExit('fixture manifest does not contain one candidate-digest host row')
token = f'"sha256": "{candidate_sha}"'.encode()
if original.count(token) != 1:
    raise SystemExit('candidate host digest token is not unique in fixture manifest bytes')
replacement = original.replace(token, f'"sha256": "{real_sha}"'.encode(), 1)
changed = json.loads(replacement)
original_host = matches[0].copy()
changed_targets = [row for row in changed['targets'] if row.get('target') == host_target]
if len(changed_targets) != 1 or changed_targets[0].get('sha256') != real_sha:
    raise SystemExit('replacement manifest does not bind the real CLI to the host row')
changed_host = changed_targets[0].copy()
original_host['sha256'] = real_sha
if original_host != changed_host:
    raise SystemExit('replacement changed another host-row field')
for row in targets:
    if row.get('target') != host_target:
        other = [item for item in changed['targets'] if item.get('target') == row.get('target')]
        if len(other) != 1 or other[0] != row:
            raise SystemExit('replacement changed a non-host target row')
original_top = {key: value for key, value in document.items() if key != 'targets'}
changed_top = {key: value for key, value in changed.items() if key != 'targets'}
if original_top != changed_top:
    raise SystemExit('replacement changed a non-target manifest field')
replacement_path.write_bytes(replacement)
receipt = {
    'candidate_sha256': candidate_sha,
    'host_target': host_target,
    'manifest_path': manifest_path,
    'original_sha256': hashlib.sha256(original).hexdigest(),
    'replacement_sha256': hashlib.sha256(replacement).hexdigest(),
    'replacement_changes_only_unique_host_sha256': True,
    'real_cli_sha256': real_sha,
}
receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n')
PY
then
  echo 'FATAL: could not prepare a host-only disposable manifest copy' >&2
  exit 1
fi
if ! cp -p "$fixture_replacement" "$fixture_install" \
  || ! mv -f "$fixture_install" "$fixture_manifest"; then
  echo 'FATAL: could not atomically install the disposable scratch manifest' >&2
  exit 1
fi
real_status=0
"$VELNOR_TEST_REAL_CLI_BIN" "$@" || real_status=$?
if ! restore_fixture_manifest; then exit 1; fi
if ! cmp -s "$fixture_original" "$fixture_manifest"; then
  echo 'FATAL: scratch manifest restoration was not byte exact' >&2
  exit 1
fi
if ! python3 - "$fixture_receipt" "$fixture_manifest" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

receipt_path, manifest_path = map(Path, sys.argv[1:])
receipt = json.loads(receipt_path.read_text())
original = (receipt_path.parent / 'original.json').read_bytes()
restored = manifest_path.read_bytes()
receipt['restored_byte_exact'] = restored == original
receipt['restored_sha256'] = hashlib.sha256(restored).hexdigest()
if not receipt['restored_byte_exact']:
    raise SystemExit('restored manifest bytes differ from the saved original')
receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n')
PY
then
  echo 'FATAL: could not record exact scratch manifest restoration' >&2
  exit 1
fi
trap - EXIT HUP INT TERM
rm -f "$fixture_install" "$fixture_manifest.restore.$$"
if [ "$real_status" -ne 0 ]; then exit "$real_status"; fi
if ! python3 - "$3/.github" "$VELNOR_TEST_REAL_CLI_SHA256" "$VELNOR_TEST_CANDIDATE_SHA256" <<'PY'
import os
from pathlib import Path
import re
import stat
import sys

root = Path(sys.argv[1])
real_sha, candidate_sha = (value.encode() for value in sys.argv[2:])
pattern = re.compile(rb'(?m)^([ \t]*VELNOR_ASSET_SHA256: )' + re.escape(real_sha) + rb'(?=\r?$)')
if root.is_symlink() or not root.is_dir():
    raise SystemExit('generated workflow directory is missing or unsafe')
changes = 0
for directory, names, files in os.walk(root, followlinks=False):
    names[:] = [name for name in names if not (Path(directory) / name).is_symlink()]
    for name in files:
        path = Path(directory) / name
        mode = path.lstat().st_mode
        if stat.S_ISLNK(mode) or not stat.S_ISREG(mode):
            continue
        data = path.read_bytes()
        rewritten, count = pattern.subn(lambda match: match.group(1) + candidate_sha, data)
        if count:
            path.write_bytes(rewritten)
            changes += count
if changes == 0:
    raise SystemExit('generated acquisition bindings contain no real host digest to translate')
PY
then
  echo 'FATAL: could not translate only generated host acquisition bindings' >&2
  exit 1
fi
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
  OUTPUT_BINDING_MANIFEST_BACKUP_DIR="$WORK/manifest backups"
  OUTPUT_BINDING_REPO_POLICY_MARKER="$WORK/candidate repo-policy calls"
  write_output_binding_candidate_wrapper "$OUTPUT_BINDING_WRAPPER" || return 2
  OUTPUT_BINDING_BINARY_SHA="$(test_file_sha256 "$OUTPUT_BINDING_WRAPPER")" || return 2
  mkdir -p "$OUTPUT_BINDING_MANIFEST_BACKUP_DIR" || return 2
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

verify_output_binding_manifest_fixture() {
  local fault="$1" retained="$2" backup_dir="$OUTPUT_BINDING_MANIFEST_BACKUP_DIR/$1"
  if ! python3 - "$backup_dir" "$retained" "$OUTPUT_BINDING_MANIFEST" \
    "$HOST_TARGET" "$CLI_SHA" "$OUTPUT_BINDING_BINARY_SHA" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

backup_dir, retained, candidate_manifest = map(Path, sys.argv[1:4])
host, real_sha, candidate_sha = sys.argv[4:]
original_path = backup_dir / 'original.json'
replacement_path = backup_dir / 'replacement.json'
receipt_path = backup_dir / 'receipt.json'
if any(path.is_symlink() or not path.is_file() for path in
       (original_path, replacement_path, receipt_path, candidate_manifest)):
    raise SystemExit('fixture evidence or candidate manifest is missing or unsafe')
original = original_path.read_bytes()
replacement = replacement_path.read_bytes()
if original != candidate_manifest.read_bytes():
    raise SystemExit('saved scratch manifest differs from the exact candidate manifest')
receipt = json.loads(receipt_path.read_text())
if receipt.get('host_target') != host or receipt.get('real_cli_sha256') != real_sha \
    or receipt.get('candidate_sha256') != candidate_sha \
    or receipt.get('replacement_changes_only_unique_host_sha256') is not True \
    or receipt.get('restored_byte_exact') is not True:
    raise SystemExit('fixture receipt does not prove the approved host-only replacement and restore')
if receipt.get('original_sha256') != hashlib.sha256(original).hexdigest() \
    or receipt.get('replacement_sha256') != hashlib.sha256(replacement).hexdigest() \
    or receipt.get('restored_sha256') != hashlib.sha256(original).hexdigest():
    raise SystemExit('fixture receipt digest does not match retained bytes')
document = json.loads(original)
matches = [row for row in document['targets'] if row.get('target') == host]
if len(matches) != 1 or matches[0].get('sha256') != candidate_sha:
    raise SystemExit('saved fixture has no unique candidate-bound host row')
token = f'"sha256": "{candidate_sha}"'.encode()
if original.count(token) != 1 or replacement != original.replace(
    token, f'"sha256": "{real_sha}"'.encode(), 1):
    raise SystemExit('replacement did not change only the unique host SHA token')
manifest_path = Path(receipt.get('manifest_path', ''))
retained = retained.resolve(strict=True)
if manifest_path.is_symlink() or manifest_path.parent.is_symlink() \
    or not manifest_path.is_file() or manifest_path.read_bytes() != original:
    raise SystemExit('scratch manifest was not restored byte-for-byte after generation')
try:
    manifest_path.relative_to(retained)
except ValueError:
    raise SystemExit('restored scratch manifest escaped the retained failed workspace')
if manifest_path.parts[-2:] != ('.velnor', 'release-manifest.json'):
    raise SystemExit('receipt does not identify the scratch release manifest')
PY
  then
    echo "FAIL: disposable manifest host replacement or exact restore was not proven for $fault" >&2
    return 1
  fi
  echo "passed host-only disposable manifest replacement and byte-exact restore: $fault"
}

expect_output_binding_fault_rejected() {
  local fault="$1" expected status=0 log retained backup_dir
  expected="$(expected_output_binding_diagnostic "$fault")" || return 2
  log="$WORK/output-binding-$fault.log"
  backup_dir="$OUTPUT_BINDING_MANIFEST_BACKUP_DIR/$fault"
  mkdir -p "$backup_dir" || return 2
  TMPDIR="$WORK/tmp workspace" \
    PATH="$STUB_BIN:$ORIGINAL_PATH" \
    VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    GITHUB_SHA="$SOURCE_SHA" \
    GITHUB_REPOSITORY=tailrocks/velnor-new \
    VELNOR_TEST_REAL_CLI_BIN="$CLI_BIN" \
    VELNOR_TEST_OUTPUT_FAULT="$fault" \
    VELNOR_TEST_BAD_DIGEST="$OUTPUT_BINDING_BAD_DIGEST" \
    VELNOR_TEST_HOST_TARGET="$HOST_TARGET" \
    VELNOR_TEST_REAL_CLI_SHA256="$CLI_SHA" \
    VELNOR_TEST_CANDIDATE_SHA256="$OUTPUT_BINDING_BINARY_SHA" \
    VELNOR_TEST_MANIFEST_BACKUP_DIR="$backup_dir" \
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
  verify_output_binding_manifest_fixture "$fault" "$retained" || return $?
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
