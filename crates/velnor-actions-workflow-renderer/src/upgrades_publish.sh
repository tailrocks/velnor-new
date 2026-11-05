# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
: "${GH_TOKEN:?}" "${VELNOR_UPGRADE_DIR:?}" "${VELNOR_UPGRADE_BRANCH:?}" "${VELNOR_DEFAULT_BRANCH:?}" "${GITHUB_REPOSITORY:?}" "${VELNOR_VERIFIED_TREE:?}"
dir=$VELNOR_UPGRADE_DIR
@VERIFIED_GH_FUNCTION@
unset GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG GIT_CONFIG_PARAMETERS GIT_CONFIG_SYSTEM GIT_TEMPLATE_DIR GIT_EXEC_PATH GIT_SSH GIT_SSH_COMMAND GIT_ASKPASS SSH_ASKPASS GIT_SSL_NO_VERIFY
@VERIFIED_GIT_CREDENTIALS@
local_git() { GH_TOKEN= GITHUB_TOKEN= git -c commit.gpgsign=false "$@"; }
# The previous step, which held no token, left the index at exactly the tree
# the verify job accepted; nothing else is ever committed.
tree=$(local_git write-tree)
[ "$tree" = "$VELNOR_VERIFIED_TREE" ] || { echo "::error::upgrade_patch_unverified: the index is not the verified tree"; exit 1; }
base=$(local_git rev-parse HEAD)
objects=$(local_git rev-parse --path-format=absolute --git-path objects)
transport=$(mktemp -d "$RUNNER_TEMP/velnor-upgrades-git.XXXXXXXX")
trap 'rm -rf "$transport"' EXIT
mkdir "$transport/template"
export GIT_ALLOW_PROTOCOL=https
git init --bare --quiet --template="$transport/template" "$transport/repo"
remote_git() { GIT_OBJECT_DIRECTORY="$objects" git --git-dir="$transport/repo" "$@"; }
remote_url="https://github.com/$GITHUB_REPOSITORY.git"
parents=(-p "$base")
if remote_git ls-remote --exit-code --heads "$remote_url" "$VELNOR_UPGRADE_BRANCH" >/dev/null 2>&1; then
  remote_git fetch --quiet --no-tags --depth=1 "$remote_url" "refs/heads/$VELNOR_UPGRADE_BRANCH"
  previous=$(remote_git rev-parse FETCH_HEAD)
  if [ "$(remote_git rev-parse "FETCH_HEAD^{tree}")" = "$tree" ]; then
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
  commit=$(local_git commit-tree "$tree" "${parents[@]}" -F "$dir/message")
  remote_git push --quiet "$remote_url" "$commit:refs/heads/$VELNOR_UPGRADE_BRANCH"
fi
number=$(velnor_gh pr list --repo "$GITHUB_REPOSITORY" --head "$VELNOR_UPGRADE_BRANCH" --state open --json number --jq '.[0].number // empty')
if [ -n "$number" ]; then
  velnor_gh pr edit "$number" --repo "$GITHUB_REPOSITORY" --body-file "$dir/summary.md"
  echo "velnor: updated upgrade pull request #$number"
else
  velnor_gh pr create --repo "$GITHUB_REPOSITORY" --base "$VELNOR_DEFAULT_BRANCH" --head "$VELNOR_UPGRADE_BRANCH" \
    --title "chore(deps): upgrade to the latest stable releases" --body-file "$dir/summary.md"
fi
