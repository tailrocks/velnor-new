
set -uo pipefail
umask 077
WORK=${WORK:?}
FAILURES=0
pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); return 1; }
expect_failure() { local name=$1; shift; if "$@"; then fail "$name accepted unsafe input"; else pass "$name"; fi; }
expect_success() { local name=$1; shift; if "$@"; then pass "$name"; else fail "$name failed"; fi; }

mkdir -p "$WORK/bin"
cat > "$WORK/bin/df" <<'SH'
#!/usr/bin/bash
if [[ -n "${SPAWN_CHILD_PID:-}" && -n "${SPAWN_CHILD_MARKER:-}" && -e "$SPAWN_CHILD_MARKER" && ! -e "$SPAWN_CHILD_PID" ]]; then
  set -m
  if [[ "${SPAWN_MODE:-}" == term-fork ]]; then
    /usr/bin/bash "$WORK/bin/term-fork-writer.sh" "$SPAWN_CHILD_FILE" "$SPAWN_FORK_PID" "$SPAWN_FORK_FILE" </dev/null >/dev/null 2>&1 &
  else
    /usr/bin/bash -c 'while :; do printf x >> "$1"; /usr/bin/sleep 0.2; done' _ "$SPAWN_CHILD_FILE" </dev/null >/dev/null 2>&1 &
  fi
  printf '%s\n' "$!" > "$SPAWN_CHILD_PID"
  set +m
fi
if [[ "${FAKE_DF_MODE:-}" == header-only && "$1" == -B1 ]]; then
  printf 'Filesystem 1-blocks Used Available Use%% Mounted on\n'; exit 0
fi
if [[ "${FAKE_DF_MODE:-}" == nonnumeric && "$1" == -B1 ]]; then
  printf 'Filesystem 1-blocks Used Available Use%% Mounted on\n/dev/root many nope values bad%% /\n'; exit 0
fi
if [[ -n "${DF_BAD_MARKER:-}" && -e "$DF_BAD_MARKER" && "$1" == -B1 ]]; then
  printf 'Filesystem 1-blocks Used Available Use%% Mounted on\n/dev/root many nope values bad%% /\n'; exit 0
fi
if [[ "$1" == -i ]]; then
  printf 'Filesystem Inodes IUsed IFree IUse%% Mounted on\n/dev/root 100000 1200 98800 2%% /\n'
else
  printf 'Filesystem 1-blocks Used Available Use%% Mounted on\n/dev/root 1000000 1200 998800 1%% /\n'
fi
SH
cat > "$WORK/bin/find" <<'SH'
#!/usr/bin/bash
printf '%s\n' "$*" >> "${FIND_LOG:-/dev/null}"
if [[ "${APPEND_MODE:-}" == inventory && "$*" == *"$FAIL_ROOT"* && "$*" == *-printf* ]]; then
  chmod 400 "$EVIDENCE/inventory-$APPEND_LABEL.tsv"
  printf 'inventory\n' >> "$APPEND_MARKER"
fi
if [[ "${APPEND_MODE:-}" == hashes && "$*" == *"$FAIL_ROOT"* && "$*" == *-printf* ]]; then
  chmod 400 "$EVIDENCE/content-hashes-export-complete.tsv"
  printf 'hashes\n' >> "$APPEND_MARKER"
fi
if [[ "${FIND_MODE:-}" == partial && "$*" == *"$FAIL_ROOT"* && "$*" == *-printf* ]]; then
  read -r dev ino links size blocks < <(/usr/bin/stat -c '%d %i %h %s %b' -- "$FAIL_FILE")
  printf '%s\0%s\0%s\0%s\0%s\0%s\0' "$dev" "$ino" "$links" "$size" "$blocks" "$FAIL_FILE"
  exit 43
fi
exec /usr/bin/find "$@"
SH
cat > "$WORK/bin/sha256sum" <<'SH'
#!/usr/bin/bash
if [[ "${SHA_MODE:-}" == partial && "${2:-}" == "$FAIL_HASH_FILE" ]]; then
  printf '%064d  %s\n' 0 "$2"; exit 43
fi
if [[ "${APPEND_MODE:-}" == hash-zero && "${2:-}" == "$FAIL_HASH_FILE" ]]; then
  printf 'hash-zero\n' >> "$APPEND_MARKER"
  chmod 400 "$EVIDENCE/content-hashes-export-complete.tsv"
fi
exec /usr/bin/sha256sum "$@"
SH
cat > "$WORK/bin/mv" <<'SH'
#!/usr/bin/bash
if [[ -n "${RACE_PARENT:-}" && "${1:-}" == -T && "${3:-}" == ./* ]]; then
  /usr/bin/mv -T -- "$RACE_PARENT" "$RACE_PARENT.moved" || exit 1
  ln -s "$RACE_OUTSIDE" "$RACE_PARENT" || exit 1
fi
exec /usr/bin/mv "$@"
SH
cat > "$WORK/bin/mbx" <<'SH'
#!/usr/bin/bash
if [[ "${1:-}" == cache ]]; then printf '{"objects":0,"action_results":0}\n'; else printf '{"savings":{"cached_compilations":0}}\n'; fi
SH
chmod 700 "$WORK/bin/"*
export PATH="$WORK/bin:/usr/bin:/bin" FIND_LOG="$WORK/find.log"
export APPEND_MARKER="$WORK/append-injections.log"
cat > "$WORK/bash-env.sh" <<'SH'
case "$0" in */sampler.sh) [[ "${5-}" == sample ]] && set -x ;; esac
SH
chmod 600 "$WORK/bash-env.sh"
export BASH_ENV="$WORK/bash-env.sh"
render_start() {
  sed -e "s|__SAMPLER_BASE64__|$(base64 -w0 "$WORK/sampler.sh")|g" \
    -e "s|__SAMPLER_SHA256__|__SAMPLER_SHA__|g" \
    -e "s|__PATH_VALIDATION_BASE64__|$(base64 -w0 "$WORK/path-validation.sh")|g" \
    -e "s|__PATH_VALIDATION_SHA256__|__PATH_VALIDATION_SHA__|g" \
    "$WORK/start.template" > "$WORK/start.sh"
  chmod 700 "$WORK/start.sh"
}
setup_case() {
  local machine_arch runtime_arch image_os image_version
  printf 'TIMING elapsed=%ss entering_case=%s\n' "$SECONDS" "$1"
  CASE_ROOT="$WORK/$1"; RUNNER_TEMP="$CASE_ROOT/runner.temp"
  EVIDENCE="$RUNNER_TEMP/mbx-cache-evidence"; GITHUB_ENV="$CASE_ROOT/github.env"
  mkdir -p "$RUNNER_TEMP"/{cargo,cache,target,shims,mbx-single-bundle/cache/cas/v1/blake3/aa}
  printf cargo > "$RUNNER_TEMP/cargo/registry.data"; printf cache > "$RUNNER_TEMP/cache/cache.data"
  : > "$RUNNER_TEMP/cache/empty.data"
  printf target > "$RUNNER_TEMP/target/target.data"; printf shim > "$RUNNER_TEMP/shims/shim.data"
  printf payload-original > "$RUNNER_TEMP/mbx-single-bundle/cache/cas/v1/blake3/aa/payload-01"
  printf bundle-unrelated > "$RUNNER_TEMP/mbx-single-bundle/unrelated.txt"
  : > "$GITHUB_ENV"
  export RUNNER_TEMP EVIDENCE GITHUB_ENV
  export GITHUB_EVENT_NAME=workflow_dispatch GITHUB_REF=refs/heads/main GITHUB_REF_PROTECTED=true
  export GITHUB_RUN_ID=701 GITHUB_RUN_ATTEMPT=1 GITHUB_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  export GITHUB_WORKFLOW_REF=owner/repo/.github/workflows/qualification.yml@refs/heads/main
  machine_arch=$(uname -m)
  case "$machine_arch" in
    x86_64) runtime_arch=X64 ;;
    aarch64|arm64) runtime_arch=ARM64 ;;
    *) fail "unsupported runtime architecture $machine_arch"; return 1 ;;
  esac
  . /etc/os-release
  [[ -n "${ID:-}" && -n "${VERSION_ID:-}" ]] || { fail 'OS release identity missing'; return 1; }
  image_os="${ID}${VERSION_ID%%.*}"
  image_version=$VERSION_ID
  export RUNNER_OS=Linux RUNNER_ARCH="$runtime_arch" ImageOS="$image_os" ImageVersion="$image_version"
  export CARGO_HOME="$RUNNER_TEMP/cargo" MBX_SELECTED_CACHE_ROOT="$RUNNER_TEMP/cache" MBX_CACHE_DIR="$RUNNER_TEMP/cache"
  export MBX_TARGET_ROOT="$RUNNER_TEMP/target" MBX_SHIMS_DIR="$RUNNER_TEMP/shims"
  export MBX_QUALIFICATION_PHASE_FILE="$EVIDENCE/phases.tsv" MBX_QUALIFICATION_SAMPLE_INTERVAL=1
  export MBX_QUALIFICATION_FINALIZER_WAIT=5 MBX_QUALIFICATION_EXPECTED_INVENTORIES=final
  export MBX_QUALIFICATION_JOB_ID=runtime-fixture MBX_QUALIFICATION_ROLE=reader
  export MBX_QUALIFICATION_ACTION_REF=owner/action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  export MBX_CACHE_SCOPE=fixture MBX_VERSION=1.22.0 RUSTUP_TOOLCHAIN=1.98.1
  export MBX_QUALIFICATION_CACHE_PRIMARY=primary MBX_QUALIFICATION_CACHE_PREFIX=prefix
  export MBX_QUALIFICATION_CACHE_MATCHED_KEY=primary MBX_QUALIFICATION_CACHE_HIT=true
  export MBX_QUALIFICATION_RESTORE_PRIMARY_KEY=primary MBX_QUALIFICATION_RESTORE_CONCLUSION=success
  export MBX_QUALIFICATION_EXPORT_READY=false MBX_QUALIFICATION_EXPORT_STATUS=0
  export MBX_QUALIFICATION_GC_STATUS=0 MBX_QUALIFICATION_SAVE_OUTCOME=success
  export MBX_QUALIFICATION_CACHE_GENERATION=generation MBX_QUALIFICATION_RUSTC_IDENTITY=rustc
  export MATCHED=primary PRIMARY=primary CACHE_HIT=true
  export MBX_QUALIFICATION_IMPORT_RECEIPT="$CASE_ROOT/import.receipt" MBX_QUALIFICATION_EXPORT_RECEIPT="$CASE_ROOT/export.receipt"
  printf 'exit_status=0\n' > "$MBX_QUALIFICATION_IMPORT_RECEIPT"
  printf 'exit_status=0\n' > "$MBX_QUALIFICATION_EXPORT_RECEIPT"
  export DF_BAD_MARKER="$CASE_ROOT/bad-df" SPAWN_CHILD_MARKER="$CASE_ROOT/spawn-child"
  export SPAWN_CHILD_PID="$CASE_ROOT/child.pid" SPAWN_CHILD_FILE="$CASE_ROOT/child.writes"
  unset FAKE_DF_MODE FIND_MODE SHA_MODE APPEND_MODE RACE_PARENT RACE_OUTSIDE SPAWN_MODE SPAWN_FORK_PID SPAWN_FORK_FILE || true
}
start_sampler() { bash "$WORK/start.sh"; }
expect_start_success() {
  local name=$1 status
  if bash -x "$WORK/start.sh" >"$CASE_ROOT/start.trace" 2>&1; then
    pass "$name"
    return 0
  else
    status=$?
    fail "$name failed"
    printf 'start_exit_status=%s\n' "$status" >&2
    grep -Ev '^\+ sed -e ' "$CASE_ROOT/start.trace" | tail -n 40 >&2
    if [[ -f "$EVIDENCE/sampler.log" ]]; then
      printf '%s\n' 'sampler.log tail:' >&2
      tail -n 60 "$EVIDENCE/sampler.log" >&2
    fi
    return "$status"
  fi
}
snapshot() { bash "$EVIDENCE/sampler.sh" "$EVIDENCE" "$RUNNER_TEMP" "$GITHUB_ENV" 1 snapshot "$1"; }
stop_sampler() { bash "$WORK/stop.sh"; }
stop_and_check_partial() {
  stop_sampler >/dev/null 2>&1 || true
  [[ -s "$EVIDENCE/qualification-status.tsv" ]] && ! grep -qx $'qualification_status\tcomplete' "$EVIDENCE/qualification-status.tsv"
}
write_valid_receipts() {
  printf 'fixture\tphase\n' >> "$EVIDENCE/phases.tsv"
  printf '{"objects":1,"action_results":1}\n' > "$EVIDENCE/mbx-cache-stats-import-step-end.json"
  printf '{"savings":{"cached_compilations":1}}\n' > "$EVIDENCE/mbx-stats-build-end.json"
  printf '0\n' > "$EVIDENCE/mbx-cache-stats-import-step-end.exit"
  printf '0\n' > "$EVIDENCE/mbx-stats-build-end.exit"
  bash "$WORK/receipt.sh"
}

render_start
source "$WORK/role-fixture.sh"
verify_resource_role_classes

setup_case evidence-existing
mkdir -m 700 "$EVIDENCE"; printf foreign > "$EVIDENCE/private.marker"; printf keep > "$EVIDENCE/sentinel"
expect_failure 'existing foreign evidence leaf' start_sampler
grep -qx keep "$EVIDENCE/sentinel" || fail 'existing evidence sentinel changed'

setup_case evidence-symlink
mkdir -m 700 "$CASE_ROOT/outside"; printf keep > "$CASE_ROOT/outside/sentinel"; ln -s "$CASE_ROOT/outside" "$EVIDENCE"
expect_failure 'symlink evidence leaf' start_sampler
grep -qx keep "$CASE_ROOT/outside/sentinel" || fail 'symlink evidence sentinel changed'

setup_case find-partial-finalizer
MBX_QUALIFICATION_EXPECTED_INVENTORIES=restore-step-end; export MBX_QUALIFICATION_EXPECTED_INVENTORIES
expect_start_success 'partial-walk finalizer fixture start' || exit 1
FAIL_ROOT="$RUNNER_TEMP/cache" FAIL_FILE="$RUNNER_TEMP/cache/cache.data"
export FAIL_ROOT FAIL_FILE
if FIND_MODE=partial snapshot find-partial-finalizer; then
  fail 'partial find exit accepted by snapshot'
else
  pass 'partial find exit rejected by snapshot'
fi
grep -q 'find_walk_failed:43' "$EVIDENCE/inventory-errors.txt" || fail 'partial find error receipt missing'
for _ in {1..40}; do
  samples=$(wc -l < "$EVIDENCE/samples.jsonl")
  (( samples >= 2 )) && break
  /usr/bin/sleep 0.2
done
reader_absent_bundle_boundary
write_valid_receipts
if stop_sampler >/dev/null 2>&1; then
  fail 'finalizer accepted failed partial find walk'
else
  pass 'finalizer rejects failed partial find walk'
fi
[[ -s "$EVIDENCE/qualification-status.tsv" ]] && \
  ! grep -qx $'qualification_status\tcomplete' "$EVIDENCE/qualification-status.tsv" || \
  fail 'partial find finalizer status was not incomplete'
grep -q 'inventory_or_snapshot_error' "$EVIDENCE/qualification-errors.txt" || \
  fail 'finalizer omitted partial find inventory error'
grep -q 'required_root_missing:restore-step-end' "$EVIDENCE/qualification-errors.txt" || \
  fail 'reader finalizer omitted absent restore bundle error'

setup_case sampler
export SPAWN_CHILD_PID="$CASE_ROOT/child.pid" SPAWN_CHILD_FILE="$CASE_ROOT/child.writes"
deferred_target_root="$MBX_TARGET_ROOT"
deferred_shims_dir="$MBX_SHIMS_DIR"
rm -rf -- "$deferred_target_root" "$deferred_shims_dir"
unset MBX_TARGET_ROOT MBX_SHIMS_DIR
expect_start_success 'start succeeds before typed roots exist' || exit 1
MBX_TARGET_ROOT="$deferred_target_root" MBX_SHIMS_DIR="$deferred_shims_dir"
export MBX_TARGET_ROOT MBX_SHIMS_DIR
mkdir -p "$MBX_TARGET_ROOT" "$MBX_SHIMS_DIR"
printf target > "$MBX_TARGET_ROOT/target.data"
FAIL_ROOT="$RUNNER_TEMP/cache" FAIL_FILE="$RUNNER_TEMP/cache/cache.data"
export FAIL_ROOT FAIL_FILE
if FIND_MODE=partial snapshot find-partial; then fail 'partial find exit accepted'; else pass 'partial find exit rejected'; fi
grep -q 'find_walk_failed:43' "$EVIDENCE/inventory-errors.txt" || fail 'find failure receipt missing'
export FAIL_HASH_FILE="$RUNNER_TEMP/cache/cache.data"
if SHA_MODE=partial snapshot export-complete; then fail 'partial sha256sum exit accepted'; else pass 'partial sha256sum exit rejected'; fi
grep -q 'sha256_failed' "$EVIDENCE/inventory-errors.txt" || fail 'hash failure receipt missing'
grep -Fq "$FAIL_HASH_FILE" "$EVIDENCE/content-hashes-export-complete.tsv" && fail 'failed hash emitted success row'
if FAKE_DF_MODE=header-only snapshot df-header-only; then fail 'header-only df accepted'; else pass 'header-only df rejected'; fi
grep -qx 'df_status=failed:2' "$EVIDENCE/df-df-header-only-bytes.txt" || fail 'header-only df status not failed'
if FAKE_DF_MODE=nonnumeric snapshot df-nonnumeric; then fail 'nonnumeric df accepted'; else pass 'nonnumeric df rejected'; fi
grep -qx 'df_status=failed:2' "$EVIDENCE/df-df-nonnumeric-bytes.txt" || fail 'nonnumeric df status not failed'
GOOD_CARGO_HOME="$CARGO_HOME"
CARGO_HOME="$RUNNER_TEMP/../runner.temp/cargo"; export CARGO_HOME
if snapshot dotdot-root; then fail 'dotdot root accepted'; else pass 'dotdot root rejected'; fi
grep -Fq "$CARGO_HOME" "$FIND_LOG" && fail 'find received dotdot root'
ln -s "$RUNNER_TEMP" "$CASE_ROOT/runner-alias"
CARGO_HOME="$CASE_ROOT/runner-alias/cargo"; export CARGO_HOME
if snapshot symlink-root; then fail 'symlink ancestor root accepted'; else pass 'symlink ancestor root rejected'; fi
grep -Fq "$CARGO_HOME" "$FIND_LOG" && fail 'find received symlink ancestor root'
CARGO_HOME="$GOOD_CARGO_HOME"; export CARGO_HOME
APPEND_LABEL=inventory-append-fail; export APPEND_LABEL
if APPEND_MODE=inventory FAIL_ROOT="$RUNNER_TEMP/cache" snapshot "$APPEND_LABEL"; then fail 'inventory append failure accepted'; else pass 'inventory append failure rejected'; fi
grep -qx 'inventory' "$APPEND_MARKER" || fail 'inventory append failure was not injected'

before_bundle="$RUNNER_TEMP/mbx-single-bundle/cache/cas/v1/blake3/aa/payload-01"
unrelated="$RUNNER_TEMP/mbx-single-bundle/unrelated.txt"
cp "$unrelated" "$CASE_ROOT/unrelated.before"
before_find_lines=$(wc -l < "$FIND_LOG")
dotdot_bundle="$RUNNER_TEMP/../runner.temp/mbx-single-bundle"
expect_failure 'corruptor dotdot bundle' bash "$WORK/corruptor.sh" "$dotdot_bundle" "$EVIDENCE" "$RUNNER_TEMP"
ln -s "$RUNNER_TEMP" "$CASE_ROOT/runner-link"
expect_failure 'corruptor symlink ancestor bundle' bash "$WORK/corruptor.sh" "$CASE_ROOT/runner-link/mbx-single-bundle" "$EVIDENCE" "$CASE_ROOT/runner-link"
after_find_lines=$(wc -l < "$FIND_LOG")
[[ "$before_find_lines" == "$after_find_lines" ]] || fail 'unsafe corruptor root reached find'
expect_success 'normal confined mutation' bash "$WORK/corruptor.sh" "$RUNNER_TEMP/mbx-single-bundle" "$EVIDENCE" "$RUNNER_TEMP"
cmp -s "$unrelated" "$CASE_ROOT/unrelated.before" || fail 'normal mutation changed unrelated bundle file'
grep -qx 'payload-original' "$before_bundle" && fail 'normal mutation did not change eligible payload'
MBX_QUALIFICATION_EXPECTED_INVENTORIES=restore-step-end; export MBX_QUALIFICATION_EXPECTED_INVENTORIES
reader_empty_bundle_boundary
stop_and_check_partial || fail 'normal mutation finalizer unexpectedly complete'
grep -q 'required_nonempty_root_missing:restore-step-end:bundle' "$EVIDENCE/qualification-errors.txt" || \
  fail 'reader finalizer accepted an empty restore bundle'

setup_case rename-race
set_cold_role new-key-writer
MBX_QUALIFICATION_EXPECTED_INVENTORIES=restore-step-end; export MBX_QUALIFICATION_EXPECTED_INVENTORIES
rm -rf -- "$RUNNER_TEMP/mbx-single-bundle"
expect_start_success 'race fixture evidence start' || exit 1
cold_absent_bundle_boundary 'new-key writer'
populate_bundle
RACE_PARENT="$RUNNER_TEMP/mbx-single-bundle/cache/cas/v1/blake3/aa"
RACE_OUTSIDE="$CASE_ROOT/outside"; mkdir -m 700 "$RACE_OUTSIDE"; printf outside > "$RACE_OUTSIDE/payload-01"
outside_before=$(stat -c '%d:%i:%s' "$RACE_OUTSIDE/payload-01")
export RACE_PARENT RACE_OUTSIDE
if bash "$WORK/corruptor.sh" "$RUNNER_TEMP/mbx-single-bundle" "$EVIDENCE" "$RUNNER_TEMP"; then fail 'rename-race corruptor reported success'; else pass 'rename-race corruptor failed closed'; fi
[[ "$(stat -c '%d:%i:%s' "$RACE_OUTSIDE/payload-01")" == "$outside_before" ]] || fail 'rename race changed outside sentinel'
[[ -s "$RACE_PARENT.moved/payload-01" ]] || fail 'pinned original parent did not retain payload'
grep -qx 'payload-original' "$RACE_PARENT.moved/payload-01" && fail 'rename race did not mutate pinned original parent'
[[ ! -e "$EVIDENCE/corrupt-mutation.tsv" ]] || fail 'rename race emitted success receipt'
unset RACE_PARENT RACE_OUTSIDE
stop_and_check_partial || fail 'rename-race finalizer unexpectedly complete'

setup_case hash-append
unset SPAWN_CHILD_PID SPAWN_CHILD_FILE
set_cold_role seed
MBX_QUALIFICATION_EXPECTED_INVENTORIES='restore-step-end export-complete'; export MBX_QUALIFICATION_EXPECTED_INVENTORIES
rm -rf -- "$RUNNER_TEMP/mbx-single-bundle"
expect_start_success 'zero-byte hash fixture start' || exit 1
cold_absent_bundle_boundary seed
populate_bundle
FAIL_HASH_FILE="$RUNNER_TEMP/cache/empty.data"; export FAIL_HASH_FILE
if APPEND_MODE=hash-zero snapshot export-complete; then fail 'zero-byte hash append failure accepted'; else pass 'zero-byte hash append failure rejected'; fi
grep -qx 'hash-zero' "$APPEND_MARKER" || fail 'zero-byte hash append failure was not injected'
grep -Fq "$FAIL_HASH_FILE" "$EVIDENCE/content-hashes-export-complete.tsv" && fail 'failed zero-byte hash emitted a success row'
write_valid_receipts
stop_and_check_partial || fail 'hash-append finalizer unexpectedly complete'
grep -q 'hash_inventory_mismatch:export-complete' "$EVIDENCE/qualification-errors.txt" || fail 'finalizer missed zero-byte hash row loss'

setup_case zero-byte-hash-positive
set_cold_role writer
MBX_QUALIFICATION_EXPECTED_INVENTORIES='restore-step-end export-complete'; export MBX_QUALIFICATION_EXPECTED_INVENTORIES
rm -rf -- "$RUNNER_TEMP/mbx-single-bundle"
expect_start_success 'zero-byte hash positive fixture start' || exit 1
cold_absent_bundle_boundary writer
populate_bundle
if snapshot export-complete; then pass 'valid export-complete hash snapshot'; else fail 'valid export-complete hash snapshot failed'; fi
awk -F '\t' -v path="$RUNNER_TEMP/cache/empty.data" \
  '$1 == "selected-cache-root" && $2 == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" && $3 == 0 && $4 == path {found=1} END {exit !found}' \
  "$EVIDENCE/content-hashes-export-complete.tsv" || fail 'zero-byte regular file hash row missing or invalid'
awk -F '\t' '$1 == "hashed_regular_file_count" && $2 ~ /^[1-9][0-9]*$/ {files=1} \
  $1 == "hashed_logical_bytes" && $2 ~ /^[1-9][0-9]*$/ {bytes=1} END {exit !(files && bytes)}' \
  "$EVIDENCE/duplicate-content-summary-export-complete.tsv" || fail 'positive hash completeness totals missing'
pass 'zero-byte regular file has valid SHA-256 row and reconciled totals'
for _ in {1..40}; do
  samples=$(wc -l < "$EVIDENCE/samples.jsonl")
  (( samples >= 2 )) && break
  /usr/bin/sleep 0.2
done
write_valid_receipts
run_miss_candidate_matrix
if stop_sampler >/dev/null 2>&1 && grep -qx $'qualification_status\tcomplete' "$EVIDENCE/qualification-status.tsv"; then
  pass 'positive export hash finalizer reaches complete'
  assert_provisional_certification
else
  fail 'positive export hash finalizer did not reach complete'
  cat "$EVIDENCE/qualification-errors.txt" >&2 2>/dev/null || true
fi

source "$WORK/session-fixture.sh"
printf 'TIMING elapsed=%ss entering_case=session-fixtures\n' "$SECONDS"

setup_case truncated-samples
unset SPAWN_CHILD_PID SPAWN_CHILD_FILE
expect_start_success 'truncated fixture evidence start' || exit 1
for _ in {1..30}; do [[ -s "$EVIDENCE/samples.jsonl" ]] && break; /usr/bin/sleep 0.1; done
printf '{"index":' >> "$EVIDENCE/samples.jsonl"
stop_and_check_partial || fail 'truncated-sample finalizer unexpectedly complete'
grep -q 'resource_samples_invalid_or_empty' "$EVIDENCE/qualification-errors.txt" || fail 'truncated sample not rejected'

setup_case complete-finalizer
unset SPAWN_CHILD_PID SPAWN_CHILD_FILE
set_hit_role reader
MBX_QUALIFICATION_EXPECTED_INVENTORIES=restore-step-end; export MBX_QUALIFICATION_EXPECTED_INVENTORIES
expect_start_success 'complete fixture evidence start' || exit 1
for _ in {1..40}; do
  samples=$(wc -l < "$EVIDENCE/samples.jsonl")
  (( samples >= 2 )) && break
  /usr/bin/sleep 0.2
done
reader_nonempty_bundle_boundary
write_valid_receipts
if jq -e '.receipt_status == "provisional" and .primary_key == "primary" and
    .derived_primary_key == .primary_key and .restore_primary_key == .primary_key and
    .restore_conclusion == "success" and .cache_hit == "true" and .matched_key == .primary_key' \
    "$EVIDENCE/cache-receipt.json" >/dev/null; then
  pass 'stock hit receipt binds derived and restored primary keys'
else
  fail 'stock hit receipt identity fields mismatch'
fi
if stop_sampler >/dev/null 2>&1 && grep -qx $'qualification_status\tcomplete' "$EVIDENCE/qualification-status.tsv"; then
  pass 'valid finalizer reaches complete'
  assert_provisional_certification
else
  fail 'valid finalizer did not reach complete'
  cat "$EVIDENCE/qualification-errors.txt" >&2 2>/dev/null || true
fi

if (( FAILURES > 0 )); then printf 'runtime fixture failures: %s\n' "$FAILURES" >&2; exit 1; fi
printf 'all runtime safety cases passed\n'
