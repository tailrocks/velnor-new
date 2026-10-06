#!/usr/bin/env bash
# Print one Actions job's step durations from the jobs API.
# Durations use the API timestamps. They are not a local monotonic clock.
set -euo pipefail
job="${1:-}"
if [ -z "$job" ]; then
  printf '%s\n' "usage: extract-job-phases.sh JOB_ID" >&2
  exit 1
fi
gh=/Users/donbeave/.local/share/mise/shims/gh
json="$(mktemp "/var/folders/8p/h376l_nn3375kyj72czdq2x80000gn/T/grok-goal-649f0cec5c91/implementer/job-phases.XXXXXX")"
trap 'rm -f "$json"' EXIT
"$gh" api "repos/ChainArgos/java-monorepo/actions/jobs/${job}" >"$json"
python3 - "$json" <<'PY'
import json, sys
from datetime import datetime
job = json.load(open(sys.argv[1]))
def parse(value):
    if not value or value.startswith("0001-"):
        return None
    return datetime.fromisoformat(value.replace("Z", "+00:00"))
started = parse(job.get("started_at"))
completed = parse(job.get("completed_at"))
print(f"job={job.get('id')} name={job.get('name')}")
print(f"status={job.get('status')} conclusion={job.get('conclusion')}")
print(f"runner={job.get('runner_name')} labels={','.join(job.get('labels') or [])}")
if started and completed:
    print(f"job_seconds={(completed - started).total_seconds():.0f}")
for step in job.get("steps") or []:
    a = parse(step.get("started_at"))
    b = parse(step.get("completed_at"))
    if a and b:
        dur = f"{(b - a).total_seconds():.0f}"
    elif a:
        dur = "running"
    else:
        dur = "pending"
    print(f"{step.get('number')}\t{dur}\t{step.get('conclusion') or '-'}\t{step.get('name')}")
PY
