#!/usr/bin/env bash
# Prepared install for the work-path daemon. This script refuses by default.
# It does not call SecKeychainItemSetAccess.
# It does not call security set-generic-password-partition-list.
# It does not write velnor-seed. It does not delete containers.
set -u
CANDIDATE="/Users/donbeave/Library/Application Support/Velnor/velnor-host.reject-5e8cc66a"
LIVE="/Users/donbeave/Library/Application Support/Velnor/velnor-host"
EXPECT_SHA="5e8cc66a400df8054779f1ad56668581d3c9b358976a3cfbaad99fe15573a83f"
LIVE_SHA="391fbaf956ab69a66ff3b9b507962ec2cbb0062f4960719f7a1d37b7ed5253d9"
echo "candidate $CANDIDATE"
echo "candidate_sha $EXPECT_SHA"
echo "candidate_cdhash b428a9559bcab6799af78b116e3a18857cad0a59"
echo "work_path /home/runner/work"
echo "forbidden SecKeychainItemSetAccess"
if [ "${VELNOR_DAEMON_INSTALL:-}" != "1" ]; then
  echo "refused: set VELNOR_DAEMON_INSTALL=1 only after Required succeeds and w7 is down"
  exit 2
fi
echo "refused: install is not authorized in this prepared script"
exit 2
