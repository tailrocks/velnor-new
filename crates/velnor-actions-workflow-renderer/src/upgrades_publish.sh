set -euo pipefail
: "${GH_TOKEN:?}" "${VELNOR_UPGRADE_DIR:?}" "${VELNOR_UPGRADE_BRANCH:?}" "${VELNOR_DEFAULT_BRANCH:?}" "${GITHUB_REPOSITORY:?}"
dir=$VELNOR_UPGRADE_DIR
velnor_gh() { mise --no-config --no-env --no-hooks exec "gh@${VELNOR_GH_VERSION:?}" -- gh "$@"; }
patch=$dir/upgrade.patch
test -s "$patch"
# The patch came from a job that ran repository code: the filter checks its
# text and the index it produces in a scratch index before the worktree or
# the real index sees a byte, then checks the real index again.
velnor_upgrade_check_patch_applies "$patch" "$dir/check.index" || exit 1
git -c core.hooksPath=/dev/null apply --index "$patch"
velnor_upgrade_check_index strict || exit 1
tree=$(git write-tree)
base=$(git rev-parse HEAD)
velnor_gh auth setup-git --hostname github.com
parents=(-p "$base")
if git ls-remote --exit-code --heads origin "$VELNOR_UPGRADE_BRANCH" >/dev/null 2>&1; then
  git fetch --quiet --no-tags --depth=1 origin "refs/heads/$VELNOR_UPGRADE_BRANCH"
  previous=$(git rev-parse FETCH_HEAD)
  if [ "$(git rev-parse "FETCH_HEAD^{tree}")" = "$tree" ]; then
    echo "velnor: $VELNOR_UPGRADE_BRANCH already carries this upgrade"
    pushed=false
  else
    # A merge of the previous upgrade head and the current default branch:
    # the branch only fast-forwards (never a force push) and the pull
    # request diff stays exactly this upgrade.
    parents=(-p "$previous" -p "$base")
    pushed=true
  fi
else
  pushed=true
fi
if [ "$pushed" = true ]; then
  export GIT_AUTHOR_NAME="Velnor Upgrades" GIT_AUTHOR_EMAIL="velnor-upgrades@users.noreply.github.com"
  export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME" GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
  {
    echo "chore(deps): upgrade to the latest stable releases"
    echo
    cat "$dir/summary.md"
    echo
    echo "Signed-off-by: $GIT_AUTHOR_NAME <$GIT_AUTHOR_EMAIL>"
  } > "$dir/message"
  commit=$(git commit-tree "$tree" "${parents[@]}" -F "$dir/message")
  git push --quiet origin "$commit:refs/heads/$VELNOR_UPGRADE_BRANCH"
fi
number=$(velnor_gh pr list --repo "$GITHUB_REPOSITORY" --head "$VELNOR_UPGRADE_BRANCH" --state open --json number --jq '.[0].number // empty')
if [ -n "$number" ]; then
  velnor_gh pr edit "$number" --repo "$GITHUB_REPOSITORY" --body-file "$dir/summary.md"
  echo "velnor: updated upgrade pull request #$number"
else
  velnor_gh pr create --repo "$GITHUB_REPOSITORY" --base "$VELNOR_DEFAULT_BRANCH" --head "$VELNOR_UPGRADE_BRANCH" \
    --title "chore(deps): upgrade to the latest stable releases" --body-file "$dir/summary.md"
fi
