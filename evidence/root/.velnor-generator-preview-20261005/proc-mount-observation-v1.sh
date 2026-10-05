#!/usr/bin/env -S -i PATH=/usr/bin:/bin /usr/bin/bash --noprofile --norc
set -Eeuo pipefail
umask 077
readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly LOG_FILE="$TASK_DIR/logs/proc-mount-observation-v1.log"
readonly RECORD_FILE="$TASK_DIR/logs/proc-mount-observation-v1.json"
[[ ! -e $LOG_FILE && ! -L $LOG_FILE && ! -e $RECORD_FILE && ! -L $RECORD_FILE ]]
started=$(/usr/bin/date -u '+%Y-%m-%dT%H:%M:%S.%NZ')
set +e
/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/unshare --mount --pid --fork -- /bin/bash --noprofile --norc -s >"$LOG_FILE" 2>&1 <<'INNER'
set -Eeuo pipefail
/usr/bin/mount --make-rprivate /
base=$(/usr/bin/mktemp -d /root/.velnor-generator-preview-20261005/proc-options-fixture.XXXXXXXX)
mkdir "$base/proc"
cleanup() {
  if /usr/bin/mountpoint -q "$base/proc"; then /usr/bin/umount "$base/proc"; fi
  rmdir "$base/proc" "$base"
}
trap cleanup EXIT
/usr/bin/mount -t proc -o ro,nosuid,nodev,noexec,hidepid=2,subset=pid proc "$base/proc"
options=$(/usr/bin/findmnt -rn -o OPTIONS --target "$base/proc")
printf 'findmnt_options=%s\n' "$options"
case ",$options," in *,hidepid=invisible,*) ;; *) echo 'expected exact kernel alias hidepid=invisible' >&2; exit 1 ;; esac
for required in ro nosuid nodev noexec subset=pid; do
  case ",$options," in *",$required,"*) ;; *) printf 'missing required mount option: %s\n' "$required" >&2; exit 1 ;; esac
done
mountinfo=$(/usr/bin/awk -v target="$base/proc" '$5 == target { print }' "$base/proc/self/mountinfo")
[[ -n $mountinfo ]]
printf 'private_mountinfo=%s\n' "$mountinfo"
case ",$mountinfo," in *,hidepid=invisible,*) ;; *) echo 'mountinfo does not report exact hidepid alias' >&2; exit 1 ;; esac
INNER
status=$?
set -e
ended=$(/usr/bin/date -u '+%Y-%m-%dT%H:%M:%S.%NZ')
log_sha=$(/usr/bin/sha256sum "$LOG_FILE" | /usr/bin/cut -d' ' -f1)
/usr/bin/python3 -I -S - "$RECORD_FILE" "$started" "$ended" "$status" "$log_sha" <<'PY'
import json
import os
import sys

path, started, ended, status, log_sha = sys.argv[1:]
record = {
    "schema": 1,
    "status": "PASS" if status == "0" else "FAIL",
    "command": "env -i PATH=/usr/bin:/bin /usr/bin/unshare --mount --pid --fork -- /bin/bash --noprofile --norc -s",
    "started_at_utc": started,
    "ended_at_utc": ended,
    "exit_code": int(status),
    "log_path": "/root/.velnor-generator-preview-20261005/logs/proc-mount-observation-v1.log",
    "log_sha256": log_sha,
    "host_kernel": os.uname().release,
    "findmnt_version": "util-linux 2.41.5",
    "requested_proc_options": ["ro", "nosuid", "nodev", "noexec", "hidepid=2", "subset=pid"],
    "scope": "Disposable private mount and PID namespaces; proc-only observation; no repository source, helper, Rustup, Mise, Cargo, credential source, or network operation.",
    "authoritative_reference": "https://docs.kernel.org/6.12/filesystems/proc.html#chapter-4-configuring-procfs",
    "authoritative_section": "4.1 Mount options; hidepid=invisible or hidepid=2 and subset=pid semantics.",
}
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
try:
    payload = (json.dumps(record, sort_keys=True, indent=2) + "\n").encode()
    os.write(fd, payload)
    os.fchmod(fd, 0o600)
finally:
    os.close(fd)
PY
exit "$status"
