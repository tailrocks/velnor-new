set -euo pipefail
evidence="$RUNNER_TEMP/mbx-cache-evidence"
bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" validate
printf '%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" "$MBX_PHASE" >> "$MBX_QUALIFICATION_PHASE_FILE"
