# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
# shellcheck shell=bash

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
if ! node - "$fixture_original" "$fixture_replacement" "$fixture_receipt" \
  "$VELNOR_TEST_HOST_TARGET" "$VELNOR_TEST_REAL_CLI_SHA256" "$VELNOR_TEST_CANDIDATE_SHA256" "$PWD/$fixture_manifest" <<'JS'
const fs = require('node:fs');
const crypto = require('node:crypto');
const { isDeepStrictEqual } = require('node:util');
const [originalPath, replacementPath, receiptPath, hostTarget, realSha, candidateSha, manifestPath] = process.argv.slice(2);
const fail = message => { throw new Error(message); };
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const parse = bytes => JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
const original = fs.readFileSync(originalPath);
if (!/^[0-9a-f]{64}$/.test(realSha) || !/^[0-9a-f]{64}$/.test(candidateSha)) fail('fixture host digests are malformed');
if (realSha === candidateSha) fail('fixture requires distinct real and wrapper host digests');
const document = parse(original);
const targets = document.targets;
if (!Array.isArray(targets)) fail('fixture manifest has no target list');
const matches = targets.filter(row => row !== null && typeof row === 'object' && !Array.isArray(row) && row.target === hostTarget);
if (matches.length !== 1 || matches[0].sha256 !== candidateSha) fail('fixture manifest does not contain one candidate-digest host row');
const token = Buffer.from(`"sha256": "${candidateSha}"`);
const replacementToken = Buffer.from(`"sha256": "${realSha}"`);
const tokenIndex = original.indexOf(token);
if (tokenIndex < 0 || original.indexOf(token, tokenIndex + 1) >= 0) fail('candidate host digest token is not unique in fixture manifest bytes');
const replacement = Buffer.concat([original.subarray(0, tokenIndex), replacementToken, original.subarray(tokenIndex + token.length)]);
const changed = parse(replacement);
const changedTargets = changed.targets.filter(row => row !== null && typeof row === 'object' && row.target === hostTarget);
if (changedTargets.length !== 1 || changedTargets[0].sha256 !== realSha) fail('replacement manifest does not bind the real CLI to the host row');
const expectedHost = { ...matches[0], sha256: realSha };
if (!isDeepStrictEqual(expectedHost, changedTargets[0])) fail('replacement changed another host-row field');
for (const row of targets) {
  if (row.target !== hostTarget) {
    const other = changed.targets.filter(item => item.target === row.target);
    if (other.length !== 1 || !isDeepStrictEqual(other[0], row)) fail('replacement changed a non-host target row');
  }
}
const top = value => Object.fromEntries(Object.entries(value).filter(([key]) => key !== 'targets'));
if (!isDeepStrictEqual(top(document), top(changed))) fail('replacement changed a non-target manifest field');
fs.writeFileSync(replacementPath, replacement);
const receipt = {
  candidate_sha256: candidateSha,
  host_target: hostTarget,
  manifest_path: manifestPath,
  original_sha256: digest(original),
  replacement_sha256: digest(replacement),
  replacement_changes_only_unique_host_sha256: true,
  real_cli_sha256: realSha,
};
fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
JS
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
if ! node - "$fixture_receipt" "$fixture_manifest" <<'JS'
const fs = require('node:fs');
const crypto = require('node:crypto');
const [receiptPath, manifestPath] = process.argv.slice(2);
const receipt = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(fs.readFileSync(receiptPath)));
const original = fs.readFileSync(`${require('node:path').dirname(receiptPath)}/original.json`);
const restored = fs.readFileSync(manifestPath);
receipt.restored_byte_exact = restored.equals(original);
receipt.restored_sha256 = crypto.createHash('sha256').update(restored).digest('hex');
if (!receipt.restored_byte_exact) throw new Error('restored manifest bytes differ from the saved original');
fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
JS
then
  echo 'FATAL: could not record exact scratch manifest restoration' >&2
  exit 1
fi
trap - EXIT HUP INT TERM
rm -f "$fixture_install" "$fixture_manifest.restore.$$"
if [ "$real_status" -ne 0 ]; then exit "$real_status"; fi
if ! node - "$3/.github" "$VELNOR_TEST_REAL_CLI_SHA256" "$VELNOR_TEST_CANDIDATE_SHA256" <<'JS'
const fs = require('node:fs');
const path = require('node:path');
const [root, realSha, candidateSha] = process.argv.slice(2);
const fail = message => { throw new Error(message); };
const real = Buffer.from(realSha);
const candidate = Buffer.from(candidateSha);
const prefix = Buffer.from('VELNOR_ASSET_SHA256: ');
const rootInfo = fs.lstatSync(root);
if (rootInfo.isSymbolicLink() || !rootInfo.isDirectory()) fail('generated workflow directory is missing or unsafe');
let changes = 0;
function visit(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    const info = fs.lstatSync(file);
    if (info.isSymbolicLink()) continue;
    if (info.isDirectory()) {
      visit(file);
      continue;
    }
    if (!info.isFile()) continue;
    const data = fs.readFileSync(file);
    const chunks = [];
    let start = 0;
    let count = 0;
    for (let end = 0; end <= data.length; end += 1) {
      if (end !== data.length && data[end] !== 10) continue;
      let lineEnd = end;
      if (lineEnd > start && data[lineEnd - 1] === 13) lineEnd -= 1;
      let fieldStart = start;
      while (fieldStart < lineEnd && (data[fieldStart] === 32 || data[fieldStart] === 9)) fieldStart += 1;
      const valueStart = fieldStart + prefix.length;
      const valueEnd = valueStart + real.length;
      if (data.subarray(fieldStart, valueStart).equals(prefix) && data.subarray(valueStart, valueEnd).equals(real)
          && valueEnd === lineEnd) {
        chunks.push(data.subarray(start, valueStart), candidate, data.subarray(valueEnd, end + (end < data.length ? 1 : 0)));
        count += 1;
      } else {
        chunks.push(data.subarray(start, end + (end < data.length ? 1 : 0)));
      }
      start = end + 1;
    }
    if (count) {
      fs.writeFileSync(file, Buffer.concat(chunks));
      changes += count;
    }
  }
}
visit(root);
if (changes === 0) fail('generated acquisition bindings contain no real host digest to translate');
JS
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
    if (fault == "missing-digest") lines[start]=indents[1] "VELNOR_ASSET_SHA256: "
    if (fault == "missing-url") lines[start+1]=indents[2] "VELNOR_ASSET_URL: "
    if (fault == "missing-commit") lines[start+2]=indents[3] "VELNOR_RELEASE_COMMIT: "
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
  local linux_sha arm_sha intel_sha real_linux_sha real_arm_sha real_intel_sha repo_root root_var=ROOT
  repo_root="${!root_var}"
  OUTPUT_BINDING_WRAPPER="$WORK/output-binding candidate"
  OUTPUT_BINDING_REPO="$WORK/output-binding repo"
  OUTPUT_BINDING_PREVIEW="$WORK/output-binding preview"
  OUTPUT_BINDING_MANIFEST="$WORK/output-binding manifest.json"
  OUTPUT_BINDING_REAL_MANIFEST="$WORK/output-binding real manifest.json"
  OUTPUT_BINDING_DUPLICATE_MANIFEST="$WORK/output-binding duplicate manifest.json"
  OUTPUT_BINDING_MANIFEST_BACKUP_DIR="$WORK/manifest backups"
  OUTPUT_BINDING_REPO_POLICY_MARKER="$WORK/candidate repo-policy calls"
  write_output_binding_candidate_wrapper "$OUTPUT_BINDING_WRAPPER" || return 2
  OUTPUT_BINDING_BINARY_SHA="$(test_file_sha256 "$OUTPUT_BINDING_WRAPPER")" || return 2
  mkdir -p "$OUTPUT_BINDING_MANIFEST_BACKUP_DIR" || return 2
  linux_sha="$TEST_LINUX_SHA"
  arm_sha="$TEST_ARM_SHA"
  intel_sha="$TEST_INTEL_SHA"
  real_linux_sha="$TEST_LINUX_SHA"
  real_arm_sha="$TEST_ARM_SHA"
  real_intel_sha="$TEST_INTEL_SHA"
  case "$HOST_TARGET_INDEX" in
    0) linux_sha="$OUTPUT_BINDING_BINARY_SHA"; real_linux_sha="$CLI_SHA" ;;
    1) arm_sha="$OUTPUT_BINDING_BINARY_SHA"; real_arm_sha="$CLI_SHA" ;;
    2) intel_sha="$OUTPUT_BINDING_BINARY_SHA"; real_intel_sha="$CLI_SHA" ;;
    *) echo "FAIL: unsupported host target index for binding test" >&2; return 1 ;;
  esac
  OUTPUT_BINDING_BAD_DIGEST="$(printf '%064d' 0 | tr '0' 'a')"
  if [ "$OUTPUT_BINDING_BAD_DIGEST" = "$linux_sha" ]; then
    OUTPUT_BINDING_BAD_DIGEST="$(printf '%064d' 0 | tr '0' 'd')"
  fi
  write_test_manifest "$OUTPUT_BINDING_MANIFEST" "$SOURCE_SHA" \
    "$linux_sha" "$arm_sha" "$intel_sha" || return 2
  OUTPUT_BINDING_MANIFEST_SHA="$(test_file_sha256 "$OUTPUT_BINDING_MANIFEST")" || return 2
  write_test_manifest "$OUTPUT_BINDING_REAL_MANIFEST" "$SOURCE_SHA" \
    "$real_linux_sha" "$real_arm_sha" "$real_intel_sha" || return 2
  OUTPUT_BINDING_REAL_MANIFEST_SHA="$(test_file_sha256 "$OUTPUT_BINDING_REAL_MANIFEST")" || return 2
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
  if [[ ! "$CLI_SHA" =~ ^[0-9a-f]{64}$ \
    || ! "$OUTPUT_BINDING_REAL_MANIFEST_SHA" =~ ^[0-9a-f]{64}$ ]] \
    || ! jq -e --arg target "$HOST_TARGET" --arg digest "$CLI_SHA" \
      --arg source "$SOURCE_SHA" '
      .commit == $source and
      ([.targets[] | select(.target == $target and .sha256 == $digest)] | length) == 1
    ' "$OUTPUT_BINDING_REAL_MANIFEST" >/dev/null; then
    echo "FAIL: real output-binding manifest does not bind the CLI digest" >&2
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

  mkdir -p "$OUTPUT_BINDING_REPO" || return 2
  cp -RP "$repo_root/fixtures/empty-suite/." "$OUTPUT_BINDING_REPO/" || return 2
  mkdir -p "$OUTPUT_BINDING_REPO/.velnor" || return 2
  cp "$OUTPUT_BINDING_REAL_MANIFEST" "$OUTPUT_BINDING_REPO/.velnor/release-manifest.json" || return 2
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
  if ! validate_output_bindings_for_manifest "$OUTPUT_BINDING_REAL_MANIFEST" "$OUTPUT_BINDING_PREVIEW"; then
    echo "FAIL: untouched real CLI preview did not pass output-binding validation" >&2
    return 1
  fi
  OUTPUT_BINDING_PREVIEW_SHA="$(output_binding_preview_fingerprint "$OUTPUT_BINDING_PREVIEW")" || return 2
  echo "candidate binary SHA=$OUTPUT_BINDING_BINARY_SHA; candidate manifest SHA=$OUTPUT_BINDING_MANIFEST_SHA"
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

validate_output_bindings_for_manifest() {
  local manifest="$1" preview="$2" status=0 output
  prepare_qualification_guard || return 2
  verify_qualification_guard || return 2
  output="$("$QUALIFICATION_GUARD_BIN" qualification-output-bindings \
    "$manifest" "$preview/.github" 2>&1)" || status=$?
  verify_qualification_guard || return 2
  if [ "$status" -ne 0 ]; then
    output="${output#archive guard: }"
    printf 'candidate output binding mismatch: %s\n' "$output"
    return 1
  fi
  return 0
}

expect_normalized_output_bindings_valid() {
  local preview="$WORK/output-binding normalized preview"
  local manifest="$WORK/output-binding normalized manifest.json"
  local digest_placeholder commit_placeholder after
  digest_placeholder="$(printf '%064d' 0 | tr '0' 'a')"
  commit_placeholder="$(printf '%040d' 0 | tr '0' 'b')"
  mkdir "$preview" || return 2
  cp -RP "$OUTPUT_BINDING_PREVIEW/." "$preview/" || return 2
  if ! jq --arg commit "$commit_placeholder" --arg digest "$digest_placeholder" \
    '.commit = $commit | .targets |= map(.sha256 = $digest)' \
    "$OUTPUT_BINDING_REAL_MANIFEST" >"$manifest"; then
    return 2
  fi
  normalize_manifest_digests "$OUTPUT_BINDING_REAL_MANIFEST" "$preview" || return 2
  if ! validate_output_bindings_for_manifest "$manifest" "$preview"; then
    echo 'FAIL: normalized copy of the real CLI preview did not validate' >&2
    return 1
  fi
  after="$(output_binding_preview_fingerprint "$OUTPUT_BINDING_PREVIEW")" || return 2
  if [ "$after" != "$OUTPUT_BINDING_PREVIEW_SHA" ]; then
    echo 'FAIL: normalized positive control changed the untouched real CLI preview' >&2
    return 1
  fi
  echo 'passed normalized positive output-binding copy'
}

verify_output_binding_manifest_fixture() {
  local fault="$1" retained="$2" backup_dir="$OUTPUT_BINDING_MANIFEST_BACKUP_DIR/$1"
  if ! node - "$backup_dir" "$retained" "$OUTPUT_BINDING_MANIFEST" \
    "$HOST_TARGET" "$CLI_SHA" "$OUTPUT_BINDING_BINARY_SHA" <<'JS'
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const [backupDir, retainedPath, candidatePath, host, realSha, candidateSha] = process.argv.slice(2);
const fail = message => { throw new Error(message); };
const readSafe = file => {
  const info = fs.lstatSync(file);
  if (info.isSymbolicLink() || !info.isFile()) fail('fixture evidence or candidate manifest is missing or unsafe');
  return fs.readFileSync(file);
};
const originalPath = path.join(backupDir, 'original.json');
const replacementPath = path.join(backupDir, 'replacement.json');
const receiptPath = path.join(backupDir, 'receipt.json');
const original = readSafe(originalPath);
const replacement = readSafe(replacementPath);
if (!original.equals(readSafe(candidatePath))) fail('saved scratch manifest differs from the exact candidate manifest');
const receipt = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(readSafe(receiptPath)));
if (receipt.host_target !== host || receipt.real_cli_sha256 !== realSha || receipt.candidate_sha256 !== candidateSha
    || receipt.replacement_changes_only_unique_host_sha256 !== true || receipt.restored_byte_exact !== true) {
  fail('fixture receipt does not prove the approved host-only replacement and restore');
}
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
if (receipt.original_sha256 !== digest(original) || receipt.replacement_sha256 !== digest(replacement)
    || receipt.restored_sha256 !== digest(original)) fail('fixture receipt digest does not match retained bytes');
const document = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(original));
const matches = document.targets.filter(row => row !== null && typeof row === 'object' && row.target === host);
if (matches.length !== 1 || matches[0].sha256 !== candidateSha) fail('saved fixture has no unique candidate-bound host row');
const token = Buffer.from(`"sha256": "${candidateSha}"`);
const index = original.indexOf(token);
if (index < 0 || original.indexOf(token, index + 1) >= 0) fail('replacement did not change only the unique host SHA token');
const expected = Buffer.concat([original.subarray(0, index), Buffer.from(`"sha256": "${realSha}"`), original.subarray(index + token.length)]);
if (!replacement.equals(expected)) fail('replacement did not change only the unique host SHA token');
const manifestPath = receipt.manifest_path;
const parentInfo = fs.lstatSync(path.dirname(manifestPath));
if (parentInfo.isSymbolicLink() || !fs.realpathSync(manifestPath).startsWith(`${fs.realpathSync(retainedPath)}${path.sep}`)
    || !readSafe(manifestPath).equals(original)) fail('scratch manifest was not restored byte-for-byte after generation');
const relative = path.relative(fs.realpathSync(retainedPath), fs.realpathSync(manifestPath));
if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) fail('restored scratch manifest escaped the retained failed workspace');
if (!manifestPath.endsWith(`${path.sep}.velnor${path.sep}release-manifest.json`)) fail('receipt does not identify the scratch release manifest');
JS
  then
    echo "FAIL: disposable manifest host replacement or exact restore was not proven for $fault" >&2
    return 1
  fi
  echo "passed host-only disposable manifest replacement and byte-exact restore: $fault"
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
  local fault="$1" preview="$2" manifest="$3" workflow="" candidate wrong_url=""
  local first_url target_record actual_target actual_digest original_workflow
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
  OUTPUT_BINDING_MUTATED_RELATIVE="${workflow#"$preview"/}"
  case "$OUTPUT_BINDING_MUTATED_RELATIVE" in
    .github/*) ;;
    *) echo "FAIL: selected output-binding workflow escaped the preview" >&2; return 1 ;;
  esac
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
    "$manifest")" || return 1
  actual_target="$(jq -er '.target' <<<"$target_record")" || return 1
  actual_digest="$(jq -er '.sha256' <<<"$target_record")" || return 1
  OUTPUT_BINDING_EXPECTED_TARGET="$actual_target"
  if [ "$fault" = wrong-target ]; then
    wrong_url="$(jq -er --arg target "$actual_target" --arg url "$first_url" --arg digest "$actual_digest" \
      '[.targets[] | select(.target != $target and .artifact != $url and .sha256 != $digest)] | .[0].artifact // error("no target has a distinct URL and digest")' \
      "$manifest")" || return 1
    OUTPUT_BINDING_EXPECTED_TARGET="$(jq -er --arg url "$wrong_url" '[.targets[] | select(.artifact == $url)] | if length == 1 then .[0].target else error("alternate target is not unique") end' "$manifest")" || return 1
    if [ "$wrong_url" = "$first_url" ] \
      || [ "$(jq -er --arg url "$wrong_url" '.targets[] | select(.artifact == $url) | .sha256' "$manifest")" = "$actual_digest" ]; then
      echo "FAIL: alternate manifest target does not have a distinct URL and digest" >&2
      return 1
    fi
  fi
  OUTPUT_BINDING_BAD_COMMIT="$(printf '%040d' 0 | tr '0' 'b')"
  if [ "$OUTPUT_BINDING_BAD_COMMIT" = "$SOURCE_SHA" ]; then
    OUTPUT_BINDING_BAD_COMMIT="$(printf '%040d' 0 | tr '0' 'c')"
  fi
  original_workflow="$WORK/output-binding-$fault.workflow.original"
  cp -p "$workflow" "$original_workflow" || return 2
  if ! awk -v fault="$fault" -v bad_digest="$OUTPUT_BINDING_BAD_DIGEST" \
    -v wrong_url="$wrong_url" -v wrong_commit="$OUTPUT_BINDING_BAD_COMMIT" '
    function key_value(line, expected,    indent, body, separator, key) {
      if (line !~ /^[ \t]*VELNOR_ASSET_SHA256: [^[:space:]]+$/ &&
          line !~ /^[ \t]*VELNOR_ASSET_URL: [^[:space:]]+$/ &&
          line !~ /^[ \t]*VELNOR_RELEASE_COMMIT: [^[:space:]]+$/) return 0
      indent=line
      sub(/[^ \t].*$/, "", indent)
      body=substr(line, length(indent) + 1)
      separator=index(body, ": ")
      key=substr(body, 1, separator - 1)
      if (key != expected) return 0
      saved_indent[expected]=indent
      return 1
    }
    { lines[NR]=$0 }
    END {
      for (i=1; i+2<=NR; i++) {
        delete saved_indent
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
  ' "$workflow" >"$workflow.mutated"; then
    rm -f "$original_workflow" "$workflow.mutated"
    return 1
  fi
  if ! mv "$workflow.mutated" "$workflow"; then
    rm -f "$original_workflow" "$workflow.mutated"
    return 1
  fi
  rm -f "$original_workflow"
}

expect_output_binding_fault_rejected() {
  local fault="$1" mutation="$WORK/output-binding-$1" copied_before before after validation_output original_after
  local expected status=0 log retained backup_dir scope mise_receipt direct_receipt
  local sed_path
  mkdir "$mutation" || return 2
  cp -RP "$OUTPUT_BINDING_PREVIEW/." "$mutation/" || return 2
  copied_before="$(output_binding_preview_fingerprint "$mutation")" || return 2
  mutate_output_binding_preview "$fault" "$mutation" "$OUTPUT_BINDING_REAL_MANIFEST" || return 2
  expected="$(expected_output_binding_diagnostic "$fault")" || return 2
  before="$(output_binding_preview_fingerprint "$mutation")" || return 2
  if [ "$copied_before" = "$before" ]; then
    echo "FAIL: fault mutation $fault did not change its preview copy" >&2
    return 1
  fi
  verify_qualification_guard || return 2
  status=0
  validation_output="$(validate_output_bindings_for_manifest \
    "$OUTPUT_BINDING_REAL_MANIFEST" "$mutation" 2>&1)" || status=$?
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

  scope="$WORK/output-binding-$fault collector"
  mkdir -p "$scope/bin" "$scope/scratch" || return 2
  sed_path="$(command -v sed)" || return 2
  printf '%s\n' "$scope" >"$scope/bin/scope" || return 2
  printf '%s\n' "$sed_path" >"$scope/bin/real-sed" || return 2
  cat >"$scope/bin/sed" <<'SED'
#!/bin/sh
base=${0%/*}
IFS= read -r real_sed <"$base/real-sed" || exit 88
IFS= read -r scope <"$base/scope" || exit 88
for argument do
  case "$argument" in
    "$scope"/scratch/velnor-goldens-work.*/stage/nested/preview/.github/*)
      printf '%s\n' "$argument" >>"$scope/normalization-reached" || exit 88
      ;;
  esac
done
exec "$real_sed" "$@"
SED
  chmod 755 "$scope/bin/sed" || return 2
  log="$scope/collector.log"
  backup_dir="$OUTPUT_BINDING_MANIFEST_BACKUP_DIR/$fault"
  mkdir -p "$backup_dir" || return 2
  mise_receipt="$scope/mise.receipt"
  direct_receipt="$scope/guard.receipt"
  : >"$mise_receipt" || return 2
  : >"$direct_receipt" || return 2
  status=0
  TMPDIR="$scope/scratch" \
    PATH="$scope/bin:$STUB_BIN:$MISE_ADAPTER_BIN:$ORIGINAL_PATH" \
    VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    VELNOR_TEST_REAL_MISE="$REAL_MISE" \
    VELNOR_TEST_ORIGINAL_PATH="$ORIGINAL_PATH" \
    VELNOR_TEST_RUST_VERSION="$RUST_VERSION" \
    VELNOR_TEST_CARGO_GUARD="$VELNOR_TEST_CARGO_GUARD" \
    VELNOR_TEST_MISE_CARGO_PATH="$MISE_CARGO_PATH" \
    VELNOR_TEST_MISE_RECEIPT="$mise_receipt" \
    VELNOR_TEST_CARGO_RECEIPT="$direct_receipt" \
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
    run_collector check-release "$OUTPUT_BINDING_WRAPPER" "$OUTPUT_BINDING_MANIFEST" \
      "$OUTPUT_BINDING_MANIFEST_SHA" >"$log" 2>&1 || status=$?
  if [ "$status" -ne 2 ] \
    || ! grep -Fq 'candidate output binding mismatch:' "$log" \
    || ! grep -Fq "$expected" "$log" \
    || ! grep -Fq 'FATAL: release candidate output bindings do not match candidate manifest' "$log"; then
    cat "$log" >&2
    echo "FAIL: malformed candidate output binding $fault was not rejected before normalization (exit $status)" >&2
    return 1
  fi
  if [ -e "$scope/normalization-reached" ] \
    || grep -Fq 'could not normalize candidate asset digests' "$log"; then
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
  if [[ -z "$retained" || "$retained" != "$scope/scratch"/velnor-goldens-work.* \
    || ! -d "$retained" ]]; then
    cat "$log" >&2
    echo "FAIL: candidate output binding $fault did not retain its failing workspace" >&2
    return 1
  fi
  verify_output_binding_manifest_fixture "$fault" "$retained" || return $?
  verify_guard_receipt "output-binding-$fault" "$mise_receipt" "$direct_receipt" || return 1
  if [ -e "$MARKER" ]; then
    echo "FAIL: output-binding fault $fault invoked a Cargo build" >&2
    return 1
  fi
  rm -rf "$retained" || return 2
  echo "passed pre-normalization independent candidate binding rejection: $fault"
}

expect_duplicate_manifest_keys_rejected() {
  local initial_fingerprint="$1" status=0 log="$WORK/duplicate-manifest-keys.log" after
  GITHUB_SHA="$SOURCE_SHA" GITHUB_REPOSITORY=tailrocks/velnor-new \
    PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    VELNOR_TEST_REAL_CLI_BIN="$CLI_BIN" \
    VELNOR_TEST_REPO_POLICY_MARKER="$OUTPUT_BINDING_REPO_POLICY_MARKER" \
    run_collector check-release "$OUTPUT_BINDING_WRAPPER" \
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
  if [ "$after" != "$initial_fingerprint" ]; then
    echo "FAIL: duplicate candidate manifest rejection changed goldens" >&2
    return 1
  fi
  echo "passed independent duplicate candidate manifest rejection"
}

run_candidate_output_binding_tests() {
  local initial_fingerprint="$1" fault
  prepare_output_binding_candidate || return $?
  mkdir -p "$WORK/tmp workspace" || return 2
  expect_normalized_output_bindings_valid || return $?
  for fault in wrong-digest wrong-commit both missing-digest missing-url missing-commit wrong-url wrong-target; do
    expect_output_binding_fault_rejected "$fault" || return $?
  done
  expect_duplicate_manifest_keys_rejected "$initial_fingerprint" || return $?
}
