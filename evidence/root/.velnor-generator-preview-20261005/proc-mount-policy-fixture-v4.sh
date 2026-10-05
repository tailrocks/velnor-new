#!/usr/bin/env -S -i PATH=/usr/bin:/bin /usr/bin/bash --noprofile --norc
set -Eeuo pipefail
umask 077
readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly NAMESPACE_SCRIPT="$TASK_DIR/preseed-preview-cargo-fetch-namespace-v6.sh"
readonly EXPECTED_NAMESPACE_SHA256=59d14f020a88558578e4abafab674705c46cf4445d3b4f492cc024338f287019
readonly LOG_FILE="$TASK_DIR/logs/proc-mount-policy-fixture-v4.log"
readonly RECORD_FILE="$TASK_DIR/logs/proc-mount-policy-fixture-v4.json"
readonly PRIOR_LOG="$TASK_DIR/logs/proc-mount-observation-v1.log"
readonly PRIOR_RECORD="$TASK_DIR/logs/proc-mount-observation-v1.json"
[[ $(/usr/bin/id -u) == 0 ]]
[[ ! -e $LOG_FILE && ! -L $LOG_FILE && ! -e $RECORD_FILE && ! -L $RECORD_FILE ]]
[[ -f $NAMESPACE_SCRIPT && ! -L $NAMESPACE_SCRIPT ]]
[[ $(/usr/bin/stat -c '%u:%g:%a' "$NAMESPACE_SCRIPT") == 0:0:700 ]]
[[ $(/usr/bin/sha256sum "$NAMESPACE_SCRIPT" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_NAMESPACE_SHA256" ]]
[[ $(/usr/bin/sha256sum "$PRIOR_LOG" | /usr/bin/cut -d' ' -f1) == 434bbf490f5ff968f3974a16eda7a731e0d54d3759f49a66c146933ebfc43063 ]]
[[ $(/usr/bin/sha256sum "$PRIOR_RECORD" | /usr/bin/cut -d' ' -f1) == a4719e2a2c972f999968a9824db219336836afbd6e9010664ad885185a30db0e ]]
started=$(/usr/bin/date -u '+%Y-%m-%dT%H:%M:%S.%NZ')
status=0
{
  printf 'namespace_sha256=%s\n' "$EXPECTED_NAMESPACE_SHA256"
  printf 'fixture_started_at_utc=%s\n' "$started"
  source <(/usr/bin/sed -n '/^proc_mount_options_are_restricted() {/,/^}/p' "$NAMESPACE_SCRIPT")
  accepted=(
    'ro,nosuid,nodev,noexec,relatime,hidepid=2,subset=pid'
    'ro,nosuid,nodev,noexec,relatime,hidepid=invisible,subset=pid'
  )
  for options in "${accepted[@]}"; do
    proc_mount_options_are_restricted "$options" || { printf 'accepted policy rejected: %s\n' "$options" >&2; exit 1; }
    printf 'accepted_policy=%s\n' "$options"
  done
  rejected=(
    'ro,nosuid,nodev,noexec,relatime,subset=pid'
    'ro,nosuid,nodev,noexec,relatime,hidepid=0,subset=pid'
    'ro,nosuid,nodev,noexec,relatime,hidepid=1,subset=pid'
    'ro,nosuid,nodev,noexec,relatime,hidepid=4,subset=pid'
    'ro,nosuid,nodev,noexec,relatime,hidepid=20,subset=pid'
    'ro,nosuid,nodev,noexec,relatime,hidepid=2,hidepid=invisible,subset=pid'
    'ro,nosuid,nodev,noexec,relatime,hidepid=2,subset=pid,subset=all'
    'ro,nosuid,nodev,noexec,relatime,hidepid=2,subset=pid,gid=0'
    'ro,nosuid,nodev,noexec,relatime,hidepid=2,subset=pid,'
    ',ro,nosuid,nodev,noexec,relatime,hidepid=2,subset=pid'
    'ro,nosuid,nodev,noexec,,relatime,hidepid=2,subset=pid'
    'rw,nosuid,nodev,noexec,relatime,hidepid=2,subset=pid'
    'ro,suid,nodev,noexec,relatime,hidepid=2,subset=pid'
    'ro,nosuid,dev,noexec,relatime,hidepid=2,subset=pid'
    'ro,nosuid,nodev,exec,relatime,hidepid=2,subset=pid'
    'ro,nosuid,nodev,noexec,relatime,hidepid=2,subset=all'
    'ro,nosuid,nodev,noexec,relatime,hidepid=2,subset=pid,unknown=1'
  )
  for options in "${rejected[@]}"; do
    if proc_mount_options_are_restricted "$options"; then
      printf 'unsafe policy accepted: %s\n' "$options" >&2
      exit 1
    fi
    printf 'rejected_policy=%s\n' "$options"
  done
  printf 'parser_matrix=PASS accepted=2 rejected=%s\n' "${#rejected[@]}"
  set +e
  /usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/unshare --mount --pid --fork -- /bin/bash --noprofile --norc -s <<'INNER'
set -Eeuo pipefail
/usr/bin/mount --make-rprivate /
base=$(/usr/bin/mktemp -d /root/.velnor-generator-preview-20261005/proc-policy-fixture.XXXXXXXX)
mkdir "$base/proc"
cleanup() {
  if /usr/bin/mountpoint -q "$base/proc"; then /usr/bin/umount "$base/proc"; fi
  rmdir "$base/proc" "$base"
}
trap cleanup EXIT
/usr/bin/mount -t proc -o ro,nosuid,nodev,noexec,hidepid=2,subset=pid proc "$base/proc"
options=$(/usr/bin/findmnt -rn -o OPTIONS --target "$base/proc")
printf 'findmnt_options=%s\n' "$options"
mountinfo=$(/usr/bin/awk -v target="$base/proc" '$5 == target { print }' "$base/proc/self/mountinfo")
[[ -n $mountinfo ]]
printf 'private_mountinfo=%s\n' "$mountinfo"
INNER
  status=$?
  set -e
  if [[ $status == 0 ]]; then
    observed_options=$(/usr/bin/awk -F= '/^findmnt_options=/ { sub(/^findmnt_options=/, ""); print; exit }' "$LOG_FILE")
    [[ -n $observed_options ]]
    proc_mount_options_are_restricted "$observed_options" || { printf 'actual proc options rejected: %s\n' "$observed_options" >&2; exit 1; }
    printf 'actual_proc_policy=PASS options=%s\n' "$observed_options"
  fi
} >"$LOG_FILE" 2>&1 || status=$?
ended=$(/usr/bin/date -u '+%Y-%m-%dT%H:%M:%S.%NZ')
log_sha=$(/usr/bin/sha256sum "$LOG_FILE" | /usr/bin/cut -d' ' -f1)
script_sha=$(/usr/bin/sha256sum "$0" | /usr/bin/cut -d' ' -f1)
/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$RECORD_FILE" "$started" "$ended" "$status" "$log_sha" "$script_sha" "$EXPECTED_NAMESPACE_SHA256" <<'PY'
import json
import os
import sys

path, started, ended, status, log_sha, script_sha, namespace_sha = sys.argv[1:]
record = {
    "schema": 1,
    "status": "PASS" if status == "0" else "FAIL",
    "command": "env -i PATH=/usr/bin:/bin /usr/bin/bash --noprofile --norc /root/.velnor-generator-preview-20261005/proc-mount-policy-fixture-v4.sh",
    "started_at_utc": started,
    "ended_at_utc": ended,
    "exit_code": int(status),
    "log_path": "/root/.velnor-generator-preview-20261005/logs/proc-mount-policy-fixture-v4.log",
    "log_sha256": log_sha,
    "fixture_script_sha256": script_sha,
    "namespace_script_sha256": namespace_sha,
    "previous_proc_observation_log_sha256": "434bbf490f5ff968f3974a16eda7a731e0d54d3759f49a66c146933ebfc43063",
    "previous_proc_observation_record_sha256": "a4719e2a2c972f999968a9824db219336836afbd6e9010664ad885185a30db0e",
    "kernel_reference": "https://docs.kernel.org/6.12/filesystems/proc.html#chapter-4-configuring-procfs",
    "kernel_reference_section": "4.1 Mount options: hidepid=invisible or hidepid=2; subset=pid",
    "scope": "Pure parser matrix plus disposable private mount and PID namespaces for proc-only option observation; no source, helper, Rustup, Mise, Cargo, credentials, or network.",
}
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
try:
    payload = (json.dumps(record, sort_keys=True, indent=2) + "\n").encode()
    os.write(fd, payload)
    os.fchmod(fd, 0o600)
finally:
    os.close(fd)
PY
if [[ $status == 0 ]]; then
  printf 'proc mount policy fixture PASS: %s\n' "$log_sha"
else
  printf 'proc mount policy fixture FAIL: exit=%s log_sha256=%s\n' "$status" "$log_sha" >&2
fi
exit "$status"
