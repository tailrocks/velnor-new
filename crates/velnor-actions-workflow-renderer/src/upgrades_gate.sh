set -euo pipefail
: "${GH_TOKEN:?}" "${GITHUB_REPOSITORY:?}" "${VELNOR_UPGRADE_BRANCH:?}" "${VELNOR_STUCK_DAYS:?}"
velnor_gh() { mise --no-config --no-env --no-hooks exec "gh@${VELNOR_GH_VERSION:?}" -- gh "$@"; }
status=0
case "${VELNOR_PREPARE_RESULT:-}" in
  success) ;;
  *) echo "::error::upgrade_prepare_failed: the upgrade job ended '${VELNOR_PREPARE_RESULT:-unknown}', so freshness is unknown"; status=1 ;;
esac
if [ "${VELNOR_SCOPE_FAILED:-}" = true ]; then
  echo "::error::upgrade_scope_needs_migration: at least one upgrade scope failed and was rolled back, so its pins stay behind; resolve it by hand:"
  # Each line is indented, so prepared text never starts a workflow command.
  printf '%s\n' "${VELNOR_SCOPE_FAILURES:-}" | sed 's/^/    /'
  status=1
fi
if [ "${VELNOR_STALE:-}" = true ]; then
  echo "::error::pins_not_latest: at least one pin is older than the latest stable release; merge the '$VELNOR_UPGRADE_BRANCH' pull request"
  status=1
fi
created=$(velnor_gh pr list --repo "$GITHUB_REPOSITORY" --head "$VELNOR_UPGRADE_BRANCH" --state open --json createdAt --jq '.[0].createdAt // empty')
if [ -n "$created" ]; then
  age_days=$(jq -nr --arg created "$created" '(now - ($created | fromdateiso8601)) / 86400 | floor')
  if [ "$age_days" -gt "$VELNOR_STUCK_DAYS" ]; then
    echo "::error::upgrade_pull_request_stuck: the '$VELNOR_UPGRADE_BRANCH' pull request has been open $age_days days (limit $VELNOR_STUCK_DAYS)"
    status=1
  fi
fi
if [ "$status" = 0 ]; then echo "velnor: every pin is the latest stable release"; fi
exit "$status"
