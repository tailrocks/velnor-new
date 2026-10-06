#!/usr/bin/env bash
# Install staged daemon 5e8cc66a only after both gates pass.
# Gate 1: CI run 37349420326 Required is success.
# Gate 2: w7 containers are gone.
# This script does not call SecKeychainItemSetAccess.
# This script does not call security set-generic-password-partition-list.
# It does not write velnor-seed. It does not delete containers.
set -eu
APP="/Users/donbeave/Library/Application Support/Velnor"
CANDIDATE="$APP/velnor-host.reject-5e8cc66a"
LIVE="$APP/velnor-host"
BAK="$APP/velnor-host.bak-391fbaf9"
STAGE="$APP/velnor-host.stage-5e8cc66a"
PREV="$APP/velnor-host.prev-before-5e8cc66a"
EXPECT="5e8cc66a400df8054779f1ad56668581d3c9b358976a3cfbaad99fe15573a83f"
LIVE_EXPECT="391fbaf956ab69a66ff3b9b507962ec2cbb0062f4960719f7a1d37b7ed5253d9"
LABEL="gui/$(id -u)/com.tailrocks.velnor.host"
GH="/Users/donbeave/.local/share/mise/shims/gh"
export DOCKER_HOST="unix:///Users/donbeave/.orbstack/run/docker.sock"

refuse() {
  echo "refused: $1"
  exit 2
}

if [ "${VELNOR_DAEMON_INSTALL:-}" != "1" ]; then
  refuse "VELNOR_DAEMON_INSTALL is not 1"
fi

w7="$(docker ps -q --filter name=w7a5eed3e3cad06ad769fedbb8c29496c | wc -l | tr -d ' ')"
if [ "$w7" != "0" ]; then
  refuse "w7 still holds the seed ($w7 containers)"
fi

producer="$("$GH" api repos/ChainArgos/java-monorepo/actions/runs/37352232818 --jq .conclusion)"
if [ "$producer" != "success" ]; then
  refuse "producer run 37352232818 is ${producer:-absent}"
fi

cd "$HOME"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
"$GH" api "repos/ChainArgos/java-monorepo/actions/runs/37349420326/jobs?per_page=100&page=1" >"$SCRIPT_DIR/required-check.json"
page2="$SCRIPT_DIR/required-check-2.json"
total="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("total_count") or 0)' "$SCRIPT_DIR/required-check.json")"
if [ "$total" -gt 100 ]; then
  "$GH" api "repos/ChainArgos/java-monorepo/actions/runs/37349420326/jobs?per_page=100&page=2" >"$page2"
else
  printf '%s\n' '{"jobs":[]}' >"$page2"
fi
req="$(python3 - "$SCRIPT_DIR/required-check.json" "$page2" <<'PY'
import json, sys
jobs=[]
for path in sys.argv[1:]:
    jobs.extend(json.load(open(path)).get("jobs") or [])
for job in jobs:
    if job.get("name")=="Required":
        print(job.get("conclusion") or job.get("status") or "absent")
        raise SystemExit
print("absent")
PY
)"
if [ "$req" != "success" ]; then
  refuse "Required is $req"
fi

live_sha="$(shasum -a 256 "$LIVE" | awk '{print $1}')"
cand_sha="$(shasum -a 256 "$CANDIDATE" | awk '{print $1}')"
bak_sha="$(shasum -a 256 "$BAK" | awk '{print $1}')"
if [ "$cand_sha" != "$EXPECT" ]; then
  refuse "candidate hash is $cand_sha"
fi
if [ "$live_sha" != "$LIVE_EXPECT" ]; then
  refuse "live hash is $live_sha"
fi
if [ "$bak_sha" != "$LIVE_EXPECT" ]; then
  refuse "backup hash is $bak_sha"
fi
if [ -e "$PREV" ] || [ -e "$STAGE" ]; then
  refuse "stage or previous file already exists"
fi
# The candidate must keep /home/runner/work and must not use /home/runner/_work.
python3 - "$CANDIDATE" <<'PY'
import sys
from pathlib import Path
data=Path(sys.argv[1]).read_bytes()
work=data.count(b"/home/runner/work")
under=data.count(b"/home/runner/_work")
if work<1 or under!=0:
    raise SystemExit("work path mismatch")
if b"SecKeychainItemSetAccess" in data:
    raise SystemExit("candidate contains SecKeychainItemSetAccess")
PY

# Same-volume stage, then mv onto the live path. Do not cp onto the live path.
cp -p "$CANDIDATE" "$STAGE"
stage_sha="$(shasum -a 256 "$STAGE" | awk '{print $1}')"
if [ "$stage_sha" != "$EXPECT" ]; then
  rm -f "$STAGE"
  refuse "stage hash is $stage_sha"
fi
ERRLOG="$HOME/Library/Logs/Velnor/host.err.log"
before_bytes="$(wc -c < "$ERRLOG" | tr -d ' ')"
old_pid="$(launchctl print "$LABEL" | awk '/pid = /{print $3; exit}')"
mv "$LIVE" "$PREV"
mv "$STAGE" "$LIVE"
launchctl kickstart -k "$LABEL"

restore() {
  if [ -f "$LIVE" ]; then
    mv "$LIVE" "$APP/velnor-host.failed-5e8cc66a"
  fi
  mv "$PREV" "$LIVE"
  launchctl kickstart -k "$LABEL"
  echo "restored live daemon $LIVE_EXPECT"
  exit 1
}

ok=0
for _ in 1 2 3 4 5 6 7 8; do
  sleep 3
  new_pid="$(launchctl print "$LABEL" | awk '/pid = /{print $3; exit}')"
  now="$(shasum -a 256 "$LIVE" | awk '{print $1}')"
  fresh="$(tail -c +$((before_bytes + 1)) "$ERRLOG" || true)"
  if [ "$now" = "$EXPECT" ] && [ -n "$new_pid" ] && [ "$new_pid" != "$old_pid" ] && printf '%s\n' "$fresh" | grep -q "status=202"; then
    ok=1
    break
  fi
done
if [ "$ok" != "1" ]; then
  restore
fi
echo "installed $EXPECT"
exit 0
