set -euo pipefail
umask 077
evidence="$RUNNER_TEMP/mbx-cache-evidence"
bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" snapshot "$MBX_PHASE"
. "$evidence/path-validation.sh"
capture_stats() {
  local base="$1" status=0
  shift
  if resource_capture_bounded "$evidence" "$base.json" "$@" 2> "$base.stderr"; then :; else
    status=$?
    printf 'exit_status=%s\n' "$status" >> "$base.stderr"
  fi
  printf '%s\n' "$status" > "$base.exit"
}
if command -v mbx >/dev/null 2>&1; then
  capture_stats "$evidence/mbx-cache-stats-$MBX_PHASE" mbx cache stats --json
  capture_stats "$evidence/mbx-stats-$MBX_PHASE" mbx stats --json
fi
printf '%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" "$MBX_PHASE-inventory-end" >> "$MBX_QUALIFICATION_PHASE_FILE"
