cat > "$WORK/bin/term-fork-writer.sh" <<'SH'
#!/usr/bin/bash
set -uo pipefail
writer_file=$1
fork_pid_file=$2
fork_file=$3
spawn_on_term() {
  trap '' TERM
  set -m
  /usr/bin/bash -c 'trap "" TERM; while :; do printf y >> "$1"; /usr/bin/sleep 0.2; done' _ "$fork_file" &
  printf '%s\n' "$!" > "$fork_pid_file"
}
trap spawn_on_term TERM
while :; do printf x >> "$writer_file"; /usr/bin/sleep 0.2; done
SH
chmod 700 "$WORK/bin/term-fork-writer.sh"

assert_live() {
  local name=$1 pid=$2
  if kill -0 "$pid" 2>/dev/null; then pass "$name remains alive"; else fail "$name was signaled"; fi
}
assert_status_not_complete() {
  [[ ! -s "$EVIDENCE/qualification-status.tsv" ]] || \
    ! grep -qx $'qualification_status\tcomplete' "$EVIDENCE/qualification-status.tsv"
}
stop_child_session() {
  local sid=$1 signal pid process_sid rows
  for signal in TERM KILL; do
    rows=$(ps -eo pid=,sid=) || return 1
    while read -r pid process_sid; do
      [[ "$process_sid" == "$sid" ]] || continue
      [[ "$pid" != "$$" && "$pid" != "$BASHPID" && "$pid" != "$PPID" ]] || continue
      kill "-$signal" -- "$pid" 2>/dev/null || true
    done <<< "$rows"
    /usr/bin/sleep 0.2
  done
}
stop_decoy() {
  kill "$decoy_pid" 2>/dev/null || true
  wait "$decoy_pid" 2>/dev/null || true
}
wait_pid_gone() {
  local name=$1 pid=$2
  for _ in {1..30}; do
    kill -0 "$pid" 2>/dev/null || { pass "$name stopped"; return 0; }
    /usr/bin/sleep 0.1
  done
  fail "$name remains after cleanup"
}
start_session_writer() {
  local case_name=$1 mode=${2:-}
  setup_case "$case_name"
  if [[ "$mode" == term-fork ]]; then
    SPAWN_MODE=term-fork SPAWN_FORK_PID="$CASE_ROOT/fork.pid" SPAWN_FORK_FILE="$CASE_ROOT/fork.writes"
    export SPAWN_MODE SPAWN_FORK_PID SPAWN_FORK_FILE
  fi
  expect_start_success "$case_name evidence start" || return 1
  : > "$SPAWN_CHILD_MARKER"
  for _ in {1..40}; do [[ -s "$SPAWN_CHILD_PID" ]] && break; /usr/bin/sleep 0.1; done
  [[ -s "$SPAWN_CHILD_PID" ]] || { fail "$case_name writer did not start"; return 1; }
  sampler_pid=$(awk -F '\t' '$1 == "pid" {print $2}' "$EVIDENCE/sampler.session.tsv")
  sampler_session_pgid=$(awk -F '\t' '$1 == "pgid" {print $2}' "$EVIDENCE/sampler.session.tsv")
  sampler_session_sid=$(awk -F '\t' '$1 == "sid" {print $2}' "$EVIDENCE/sampler.session.tsv")
  sampler_start_ticks=$(awk -F '\t' '$1 == "start_ticks" {print $2}' "$EVIDENCE/sampler.session.tsv")
  child_pid=$(cat "$SPAWN_CHILD_PID")
  child_session_sid=$(ps -o sid= -p "$child_pid" | tr -d ' ')
  child_pgid=$(ps -o pgid= -p "$child_pid" | tr -d ' ')
  [[ "$child_session_sid" == "$sampler_session_sid" ]] || fail "$case_name writer escaped sampler session"
  [[ "$child_pgid" != "$sampler_session_pgid" ]] || fail "$case_name writer lacks separate PGID"
  /usr/bin/sleep 0.3
  /usr/bin/sleep 1200 &
  decoy_pid=$!
  decoy_session_sid=$(ps -o sid= -p "$decoy_pid" | tr -d ' ')
  [[ "$decoy_session_sid" != "$sampler_session_sid" ]] || fail "$case_name decoy shares sampler SID"
}
assert_valid_session_receipt() {
  local file="$EVIDENCE/sampler.session.tsv" rows key expected actual count
  rows=$(awk 'END {print NR}' "$file")
  [[ "$rows" == 10 ]] || fail "session receipt has $rows rows, expected ten"
  awk -F '\t' 'NF != 2 || $1 == "" || $2 == "" {bad=1} END {exit bad}' "$file" || \
    fail 'valid session receipt has malformed fields'
  for key in pid pgid sid start_ticks run_id run_attempt job_id uid gid evidence_identity; do
    count=$(awk -F '\t' -v key="$key" '$1 == key {n++} END {print n+0}' "$file")
    [[ "$count" == 1 ]] || fail "valid session receipt key is not unique: $key"
  done
  for key in pid pgid sid start_ticks; do
    expected=$(case "$key" in
      pid) printf '%s' "$sampler_pid" ;;
      pgid) printf '%s' "$sampler_session_pgid" ;;
      sid) printf '%s' "$sampler_session_sid" ;;
      start_ticks) awk -F '\t' '$1 == "start_ticks" {print $2}' "$file" ;;
    esac)
    actual=$(awk -F '\t' -v key="$key" '$1 == key {print $2}' "$file")
    [[ "$actual" == "$expected" ]] || fail "session receipt identity mismatch: $key"
  done
  for key in run_id run_attempt job_id uid gid evidence_identity; do
    case "$key" in
      run_id) expected=$GITHUB_RUN_ID ;;
      run_attempt) expected=$GITHUB_RUN_ATTEMPT ;;
      job_id) expected=$MBX_QUALIFICATION_JOB_ID ;;
      uid) expected=$(id -u) ;;
      gid) expected=$(id -g) ;;
      evidence_identity) expected=$(cat "$EVIDENCE/private.identity") ;;
    esac
    actual=$(awk -F '\t' -v key="$key" '$1 == key {print $2}' "$file")
    [[ "$actual" == "$expected" ]] || fail "session receipt context mismatch: $key"
  done
  pass 'session receipt has ten unique bound fields'
}
expect_rejected_without_signal() {
  local name=$1 sampler_expected=$2 preserve_session=${3:-false} marker_expected=${4:-absent} before after
  write_valid_receipts
  if stop_sampler > "$CASE_ROOT/stop.log" 2>&1; then fail "$name was accepted"; else pass "$name rejected"; fi
  if [[ "$marker_expected" == present ]]; then
    [[ -e "$EVIDENCE/sampler.stop" ]] || fail "$name did not reach owned-session shutdown"
  else
    [[ ! -e "$EVIDENCE/sampler.stop" ]] || fail "$name created stop marker before identity validation"
  fi
  if [[ "$sampler_expected" == alive ]]; then assert_live "$name sampler" "$sampler_pid"; fi
  assert_live "$name same-session writer" "$child_pid"
  before=$(wc -c < "$SPAWN_CHILD_FILE")
  /usr/bin/sleep 0.4
  after=$(wc -c < "$SPAWN_CHILD_FILE")
  (( after > before )) || fail "$name signaled the same-session writer"
  assert_live "$name external decoy" "$decoy_pid"
  assert_status_not_complete || fail "$name reported qualification complete"
  if [[ "$preserve_session" != true ]]; then
    stop_child_session "$sampler_session_sid"
    stop_decoy
  fi
}
install_ps_failure() {
  local mode=$1
  cat > "$WORK/bin/ps" <<'SH'
#!/usr/bin/bash
case "${PS_FAILURE_MODE:-}" in
  leader-empty)
    if [[ "${1-}" == -p && -e "$PS_FAILURE_ARM" ]]; then
      printf '%s\n' leader-empty >> "$PS_FAILURE_LOG"
      exit 43
    fi
    ;;
  leader-partial)
    if [[ "${1-}" == -p && -e "$PS_FAILURE_ARM" ]]; then
      printf 'bash %s/sampler.sh\n' "$EVIDENCE"
      printf '%s\n' leader-partial >> "$PS_FAILURE_LOG"
      exit 43
    fi
    ;;
  scan-empty)
    if [[ "${1-}" == -eo && -e "$PS_FAILURE_ARM" && -e "$EVIDENCE/sampler.stop" ]]; then
      printf '%s\n' scan-empty >> "$PS_FAILURE_LOG"
      exit 43
    fi
    ;;
  scan-partial)
    if [[ "${1-}" == -eo && -e "$PS_FAILURE_ARM" && -e "$EVIDENCE/sampler.stop" ]]; then
      printf '%s %s %s %s\n' "$PS_FAILURE_UID" "$PS_FAILURE_PID" \
        "$PS_FAILURE_PGID" "$PS_FAILURE_SID"
      printf '%s\n' scan-partial >> "$PS_FAILURE_LOG"
      exit 43
    fi
    ;;
esac
exec /usr/bin/ps "$@"
SH
  chmod 700 "$WORK/bin/ps"
  export PS_FAILURE_MODE="$mode" PS_FAILURE_ARM="$CASE_ROOT/ps-failure-arm"
  export PS_FAILURE_LOG="$CASE_ROOT/ps-failure.log"
  export PS_FAILURE_UID="$(id -u)" PS_FAILURE_PID="$sampler_pid"
  export PS_FAILURE_PGID="$sampler_session_pgid" PS_FAILURE_SID="$sampler_session_sid"
  : > "$PS_FAILURE_ARM"
}
clear_ps_failure() {
  rm -f "$WORK/bin/ps"
  unset PS_FAILURE_MODE PS_FAILURE_ARM PS_FAILURE_LOG PS_FAILURE_UID PS_FAILURE_PID \
    PS_FAILURE_PGID PS_FAILURE_SID
}
assert_ps_failure_result() {
  local name=$1 status
  [[ -s "$PS_FAILURE_LOG" ]] || fail "$name did not invoke injected ps failure"
  [[ -s "$EVIDENCE/qualification-errors.txt" ]] || fail "$name omitted failure diagnostics"
  grep -qx $'qualification_status\tincomplete' "$EVIDENCE/qualification-status.tsv" || \
    fail "$name did not record incomplete qualification"
  status=$(awk -F '\t' '$1 == "shutdown_deadline_status" {print $2}' "$EVIDENCE/sampler-stop.tsv")
  [[ -n "$status" && "$status" != within_budget ]] || fail "$name accepted failed ps status: $status"
  [[ ! -e "$EVIDENCE/inventory-final.tsv" && ! -e "$EVIDENCE/root-status-final.tsv" ]] || \
    fail "$name captured a final snapshot after ps failure"
  assert_status_not_complete || fail "$name reported qualification complete"
}
run_ps_failure_case() {
  local mode=$1 marker_expected=absent sampler_expected=alive
  start_session_writer "ps-failure-$mode" || exit 1
  install_ps_failure "$mode"
  export MBX_QUALIFICATION_FINALIZER_WAIT=1
  if [[ "$mode" == scan-* ]]; then marker_expected=present; fi
  expect_rejected_without_signal "ps failure $mode" "$sampler_expected" true "$marker_expected"
  assert_ps_failure_result "ps failure $mode"
  clear_ps_failure
  stop_child_session "$sampler_session_sid"
  stop_decoy
  wait_pid_gone "ps failure $mode writer cleanup" "$child_pid"
  export MBX_QUALIFICATION_FINALIZER_WAIT=5
}

setup_case pid-validator
expect_start_success 'PID validator evidence start' || exit 1
source "$WORK/path-validation.sh"
for pid_value in 10 1000; do
  if valid_session_leader_pid "$pid_value"; then pass "valid session PID $pid_value"; else fail "valid session PID $pid_value rejected"; fi
done
for pid_value in 0 1 0100 malformed; do
  if valid_session_leader_pid "$pid_value"; then fail "invalid session PID $pid_value accepted"; else pass "invalid session PID $pid_value rejected"; fi
done
stop_and_check_partial || fail 'PID validator sampler did not stop'

start_session_writer process-group || exit 1
assert_valid_session_receipt
write_valid_receipts
if stop_sampler > "$CASE_ROOT/stop.log" 2>&1; then
  pass 'separate-PGID writer finalizer succeeded'
  grep -qx $'qualification_status\tcomplete' "$EVIDENCE/qualification-status.tsv" || fail 'successful session cleanup did not qualify'
else
  pass 'separate-PGID writer finalizer stayed incomplete after bounded cleanup'
  assert_status_not_complete || fail 'failed session cleanup reported complete'
fi
grep -qx $'remaining_session_members\t0' "$EVIDENCE/sampler-stop.tsv" || fail 'stop receipt does not prove empty session'
before_child_bytes=$(wc -c < "$SPAWN_CHILD_FILE")
/usr/bin/sleep 0.5
after_child_bytes=$(wc -c < "$SPAWN_CHILD_FILE")
[[ "$before_child_bytes" == "$after_child_bytes" ]] || fail 'same-session separate-PGID writer survived stop'
wait_pid_gone 'same-session separate-PGID writer' "$child_pid"
if kill -0 "$decoy_pid" 2>/dev/null; then pass 'unrelated SID decoy survived session stop'; else fail 'session stop signaled unrelated SID decoy'; fi
stop_decoy

start_session_writer malformed-session-controls || exit 1
valid_session_file="$CASE_ROOT/session.valid.tsv"
cp "$EVIDENCE/sampler.session.tsv" "$valid_session_file"
restore_session_control() { cp "$valid_session_file" "$EVIDENCE/sampler.session.tsv"; }

printf 'sid\t%s\n' "$sampler_session_sid" >> "$EVIDENCE/sampler.session.tsv"
expect_rejected_without_signal 'duplicate session key' alive true

restore_session_control
awk -F '\t' 'BEGIN {OFS="\t"} $1 == "pid" {print $1, "", $2; next} {print}' \
  "$EVIDENCE/sampler.session.tsv" > "$EVIDENCE/session-rewrite.tsv"
mv -- "$EVIDENCE/session-rewrite.tsv" "$EVIDENCE/sampler.session.tsv"
expect_rejected_without_signal 'empty session control field' alive true

restore_session_control
awk -F '\t' '$1 == "job_id" {printf "%s\t%s\t\n", $1, $2; next} {print}' \
  "$EVIDENCE/sampler.session.tsv" > "$EVIDENCE/session-rewrite.tsv"
mv -- "$EVIDENCE/session-rewrite.tsv" "$EVIDENCE/sampler.session.tsv"
expect_rejected_without_signal 'trailing session control tab' alive true

restore_session_control
printf 'unknown\tvalue' >> "$EVIDENCE/sampler.session.tsv"
expect_rejected_without_signal 'unterminated unknown session row' alive true

restore_session_control
awk -F '\t' 'BEGIN {OFS="\t"} $1 == "start_ticks" {$2="malformed"} {print}' \
  "$EVIDENCE/sampler.session.tsv" > "$EVIDENCE/session-rewrite.tsv"
mv -- "$EVIDENCE/session-rewrite.tsv" "$EVIDENCE/sampler.session.tsv"
expect_rejected_without_signal 'malformed start ticks' alive true

restore_session_control
wrong_start_ticks=$((sampler_start_ticks + 1))
awk -F '\t' -v wrong="$wrong_start_ticks" 'BEGIN {OFS="\t"} $1 == "start_ticks" {$2=wrong} {print}' \
  "$EVIDENCE/sampler.session.tsv" > "$EVIDENCE/session-rewrite.tsv"
mv -- "$EVIDENCE/session-rewrite.tsv" "$EVIDENCE/sampler.session.tsv"
expect_rejected_without_signal 'mismatched start ticks' alive true

restore_session_control
awk -F '\t' -v sid="$decoy_session_sid" 'BEGIN {OFS="\t"} $1 == "sid" {$2=sid} {print}' \
  "$EVIDENCE/sampler.session.tsv" > "$EVIDENCE/session-rewrite.tsv"
mv -- "$EVIDENCE/session-rewrite.tsv" "$EVIDENCE/sampler.session.tsv"
expect_rejected_without_signal 'mismatched session ID' alive true

restore_session_control
printf '%s\n' "$decoy_pid" > "$EVIDENCE/sampler.pid"
expect_rejected_without_signal 'mismatched leader PID' alive true

restore_session_control
printf '%s\n' "$sampler_pid" > "$EVIDENCE/sampler.pid"
kill -KILL -- "$sampler_pid" 2>/dev/null || true
wait_pid_gone 'sampler leader before Stop' "$sampler_pid"
expect_rejected_without_signal 'missing leader with same-session child' absent

start_session_writer term-resistant-fork term-fork || exit 1
write_valid_receipts
stop_started=$(/usr/bin/date +%s)
stop_sampler > "$CASE_ROOT/stop.log" 2>&1 &
stop_pid=$!
stop_marker_seen=0
fork_seen=0
printf 'TIMING elapsed=%ss term-resistant-stop-invoked\n' "$SECONDS"
for _ in {1..120}; do [[ -s "$SPAWN_FORK_PID" ]] && break; /usr/bin/sleep 0.1; done
[[ -s "$SPAWN_FORK_PID" ]] || fail 'TERM-resistant writer did not fork same-session child'
fork_seen=1
fork_pid=$(cat "$SPAWN_FORK_PID")
fork_session_sid=$(ps -o sid= -p "$fork_pid" | tr -d ' ')
fork_pgid=$(ps -o pgid= -p "$fork_pid" | tr -d ' ')
[[ "$fork_session_sid" == "$sampler_session_sid" ]] || fail 'TERM fork escaped sampler session'
[[ "$fork_pgid" != "$sampler_session_pgid" ]] || fail 'TERM fork retained leader process group'
printf 'TIMING elapsed=%ss term-handler-fork-created\n' "$SECONDS"
for _ in {1..300}; do
  if (( stop_marker_seen == 0 )) && [[ -e "$EVIDENCE/sampler.stop" ]]; then
    stop_marker_seen=1
    printf 'TIMING elapsed=%ss sampler-stop-marker-created\n' "$SECONDS"
  fi
  jobs -pr | grep -qx "$stop_pid" || break
  /usr/bin/sleep 0.1
done
if wait "$stop_pid"; then term_status=0; else term_status=$?; fi
stop_finished=$(/usr/bin/date +%s)
stop_elapsed=$((stop_finished - stop_started))
printf 'TIMING elapsed=%ss stop-returned duration=%ss status=%s marker=%s fork=%s\n' \
  "$SECONDS" "$stop_elapsed" "$term_status" "$stop_marker_seen" "$fork_seen"
(( stop_elapsed >= 5 && stop_elapsed < 30 )) || fail "TERM/KILL cleanup exceeded bound or skipped TERM wait: ${stop_elapsed}s"
(( stop_marker_seen == 1 )) || fail 'TERM-resistant fixture did not reach owned-session stop path'
remaining_session_members=$(awk -F '\t' '$1 == "remaining_session_members" {print $2}' "$EVIDENCE/sampler-stop.tsv")
shutdown_status=$(awk -F '\t' '$1 == "shutdown_deadline_status" {print $2}' "$EVIDENCE/sampler-stop.tsv")
shutdown_budget=$(awk -F '\t' '$1 == "shutdown_budget_seconds" {print $2}' "$EVIDENCE/sampler-stop.tsv")
shutdown_elapsed=$(awk -F '\t' '$1 == "shutdown_elapsed_centiseconds" {print $2}' "$EVIDENCE/sampler-stop.tsv")
graceful_elapsed=$(awk -F '\t' '$1 == "shutdown_graceful_elapsed_centiseconds" {print $2}' "$EVIDENCE/sampler-stop.tsv")
term_elapsed=$(awk -F '\t' '$1 == "shutdown_term_elapsed_centiseconds" {print $2}' "$EVIDENCE/sampler-stop.tsv")
kill_elapsed=$(awk -F '\t' '$1 == "shutdown_kill_elapsed_centiseconds" {print $2}' "$EVIDENCE/sampler-stop.tsv")
if [[ "$shutdown_budget" =~ ^[0-9]+$ && "$shutdown_elapsed" =~ ^[0-9]+$ &&
    "$graceful_elapsed" =~ ^[0-9]+$ && "$term_elapsed" =~ ^[0-9]+$ &&
    "$kill_elapsed" =~ ^[0-9]+$ ]]; then
  (( 10#$shutdown_budget == 20 && 10#$shutdown_elapsed <= 10#$shutdown_budget * 100 &&
    10#$graceful_elapsed <= 500 && 10#$term_elapsed > 0 && 10#$term_elapsed <= 500 &&
    10#$kill_elapsed > 0 && 10#$kill_elapsed <= 1000 &&
    10#$graceful_elapsed + 10#$term_elapsed + 10#$kill_elapsed <= 10#$shutdown_elapsed )) || \
    fail 'TERM/KILL timing receipt exceeded the configured total or phase bound'
else
  fail 'TERM/KILL timing receipt contains malformed centiseconds'
fi
[[ "$remaining_session_members" == 0 ]] || fail "TERM/KILL left owned session members: $remaining_session_members"
[[ "$shutdown_status" == expired:graceful ]] || fail "TERM/KILL graceful expiry was not recorded: $shutdown_status"
(( term_status != 0 )) || fail 'graceful expiry returned a successful Stop status'
assert_status_not_complete || fail 'graceful expiry reported qualification complete'
[[ ! -e "$EVIDENCE/inventory-final.tsv" && ! -e "$EVIDENCE/root-status-final.tsv" ]] || \
  fail 'graceful expiry captured a final snapshot'
pass 'TERM/KILL emptied owned SID within total budget but graceful expiry stayed incomplete'
wait_pid_gone 'TERM-resistant writer' "$child_pid"
wait_pid_gone 'TERM fork child' "$fork_pid"
if kill -0 "$decoy_pid" 2>/dev/null; then pass 'external SID decoy survived TERM/KILL'; else fail 'TERM/KILL signaled external SID decoy'; fi
stop_decoy

run_ps_failure_case leader-empty
run_ps_failure_case leader-partial
run_ps_failure_case scan-empty
run_ps_failure_case scan-partial

start_session_writer slow-ps-deadline || exit 1
cat > "$WORK/bin/ps" <<'SH'
#!/usr/bin/bash
if [[ "${1-}" == -p && -n "${SLOW_PS_MARKER:-}" && -e "$SLOW_PS_MARKER" ]]; then
  printf 'blocked leader check\n' >> "$SLOW_PS_LOG"
  exec /usr/bin/sleep 30
fi
exec /usr/bin/ps "$@"
SH
chmod 700 "$WORK/bin/ps"
export SLOW_PS_MARKER="$CASE_ROOT/block-ps" SLOW_PS_LOG="$CASE_ROOT/ps.log"
: > "$SLOW_PS_MARKER"
export MBX_QUALIFICATION_FINALIZER_WAIT=1
source "$WORK/path-validation.sh"
slow_stop_started=$(resource_monotonic_centiseconds)
expect_rejected_without_signal 'slow leader ps deadline' alive true
slow_stop_finished=$(resource_monotonic_centiseconds)
slow_stop_elapsed=$((slow_stop_finished - slow_stop_started))
(( slow_stop_elapsed <= 200 )) || fail "slow leader check exceeded deadline bound: ${slow_stop_elapsed}cs"
[[ "$(cat "$SLOW_PS_LOG")" == 'blocked leader check' ]] || fail 'slow ps injection did not reach leader check'
grep -qx $'shutdown_deadline_status\texpired:graceful_leader' "$EVIDENCE/sampler-stop.tsv" || \
  fail 'slow leader timeout was not recorded as expired:graceful_leader'
grep -qx $'remaining_session_members\tunknown' "$EVIDENCE/sampler-stop.tsv" || \
  fail 'slow leader timeout reported a member scan result'
[[ ! -e "$EVIDENCE/inventory-final.tsv" && ! -e "$EVIDENCE/root-status-final.tsv" ]] || \
  fail 'slow leader timeout captured a final snapshot'
assert_status_not_complete || fail 'slow leader timeout reported qualification complete'
stop_child_session "$sampler_session_sid"
stop_decoy
wait_pid_gone 'slow-ps same-session writer cleanup' "$child_pid"
[[ ! -e "$EVIDENCE/sampler.stop" ]] || fail 'slow leader timeout created sampler stop marker'
unset SLOW_PS_MARKER SLOW_PS_LOG
rm -f "$WORK/bin/ps"
export MBX_QUALIFICATION_FINALIZER_WAIT=5
printf 'TIMING slow_leader_check elapsed=%scs status=expired:graceful_leader\n' "$slow_stop_elapsed"
