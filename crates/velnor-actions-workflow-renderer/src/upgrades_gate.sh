set -euo pipefail
: "${GH_TOKEN:?}" "${GITHUB_REPOSITORY:?}" "${VELNOR_UPGRADE_BRANCH:?}" "${VELNOR_STUCK_DAYS:?}"
velnor_gh() { mise --no-config --no-env --no-hooks exec "gh@${VELNOR_GH_VERSION:?}" -- gh "$@"; }
status=0
case "${VELNOR_PREPARE_RESULT:-}" in
  success) ;;
  *) echo "::error::upgrade_prepare_failed: The upgrade job ended with result '${VELNOR_PREPARE_RESULT:-unknown}'. Freshness is unknown."; status=1 ;;
esac
if [ "${VELNOR_STALE:-}" = true ]; then
  echo "::error::pins_not_latest: At least one pin needs an update. Review and merge the '$VELNOR_UPGRADE_BRANCH' pull request."
  status=1
fi
created=$(velnor_gh pr list --repo "$GITHUB_REPOSITORY" --head "$VELNOR_UPGRADE_BRANCH" --state open --json createdAt --jq '.[0].createdAt // empty')
if [ -n "$created" ]; then
  age_days=$(( ( $(date -u +%s) - $(date -u -d "$created" +%s) ) / 86400 ))
  if [ "$age_days" -gt "$VELNOR_STUCK_DAYS" ]; then
    echo "::error::upgrade_pull_request_stuck: The '$VELNOR_UPGRADE_BRANCH' pull request has been open for $age_days days. The limit is $VELNOR_STUCK_DAYS days."
    status=1
  fi
fi
if [ "$status" = 0 ]; then echo "velnor: The upgrade freshness checks passed."; fi
exit "$status"
