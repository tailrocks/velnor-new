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
  local name=$1 sampler_expected=$2 preserve_session=${3:-false} before after
  write_valid_receipts
  if stop_sampler > "$CASE_ROOT/stop.log" 2>&1; then fail "$name was accepted"; else pass "$name rejected"; fi
  [[ ! -e "$EVIDENCE/sampler.stop" ]] || fail "$name created stop marker before identity validation"
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
term_seen=0
kill_seen=0
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
  if (( term_seen == 0 )) && grep -Fq 'sending TERM to validated members' "$EVIDENCE/qualification-errors.txt"; then
    term_seen=1
    printf 'TIMING elapsed=%ss stop-issued-TERM\n' "$SECONDS"
  fi
  if (( kill_seen == 0 )) && grep -Fq 'sending KILL to validated members' "$EVIDENCE/qualification-errors.txt"; then
    kill_seen=1
    printf 'TIMING elapsed=%ss stop-issued-KILL\n' "$SECONDS"
  fi
  jobs -pr | grep -qx "$stop_pid" || break
  /usr/bin/sleep 0.1
done
if wait "$stop_pid"; then term_status=0; else term_status=$?; fi
stop_finished=$(/usr/bin/date +%s)
stop_elapsed=$((stop_finished - stop_started))
printf 'TIMING elapsed=%ss stop-returned duration=%ss status=%s marker=%s fork=%s TERM=%s KILL=%s\n' \
  "$SECONDS" "$stop_elapsed" "$term_status" "$stop_marker_seen" "$fork_seen" "$term_seen" "$kill_seen"
(( stop_elapsed >= 5 && stop_elapsed < 30 )) || fail "TERM/KILL cleanup exceeded bound or skipped TERM wait: ${stop_elapsed}s"
if (( stop_marker_seen != 1 || term_seen != 1 || kill_seen != 1 )); then
  fail 'TERM/KILL phase markers were not all observed'
  cat "$EVIDENCE/qualification-errors.txt" >&2 2>/dev/null || true
  cat "$EVIDENCE/sampler-stop.tsv" >&2 2>/dev/null || true
fi
grep -q 'exceeded stop deadline' "$EVIDENCE/qualification-errors.txt" || fail 'TERM-resistant fixture did not reach session shutdown path'
remaining_session_members=$(awk -F '\t' '$1 == "remaining_session_members" {print $2}' "$EVIDENCE/sampler-stop.tsv")
if [[ "$remaining_session_members" == 0 ]]; then
  pass 'TERM-resistant fork ended with empty owned session'
  if (( term_status == 0 )); then
    grep -qx $'qualification_status\tcomplete' "$EVIDENCE/qualification-status.tsv" || fail 'empty TERM/KILL session did not qualify'
  else
    pass 'bounded TERM/KILL cleanup reported incomplete after intervention'
    assert_status_not_complete || fail 'nonzero TERM/KILL result reported complete'
  fi
  wait_pid_gone 'TERM-resistant writer' "$child_pid"
  wait_pid_gone 'TERM fork child' "$fork_pid"
elif (( term_status != 0 )) && assert_status_not_complete; then
  pass "TERM-resistant fork residual reported incomplete: $remaining_session_members"
else
  fail "TERM-resistant fork left $remaining_session_members members without fail-closed status"
fi
if kill -0 "$decoy_pid" 2>/dev/null; then pass 'external SID decoy survived TERM/KILL'; else fail 'TERM/KILL signaled external SID decoy'; fi
stop_decoy
