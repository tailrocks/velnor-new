#!/bin/sh
# Records what the legacy plan-v1 op does for each case here, and the reduced repository the helper plans.
# Usage: record.sh <velnor-actions binary built from this repository> <clone of tailrocks/velnor-new> <dir holding the pinned mise> [case ...]
# A case holds request.json (a pull_request, fork or local plan request; the lane inventory and named check lanes
# are those of ci.yml at its head) and, for a local case, local/append/<path> (text appended to a tracked file) and
# local/untracked/<path> (a new file); every file under local/ and snapshot/ carries a .fixture suffix. The op plans a detached worktree of the head with that change applied.
# It writes expected_outputs (the step outputs minus `matrix` and `qualification_cache_directives`, which carry task
# digests the helper does not compute) and snapshot/ (see snapshot.py).
set -eu
engine=$1
clone=$2
mise_dir=$3
shift 3
here=$(cd "$(dirname "$0")" && pwd)
for dir in "$here"/*/; do
  name=$(basename "$dir")
  if [ "$#" -gt 0 ]; then case " $* " in *" $name "*) ;; *) continue ;; esac; fi
  work=$(mktemp -d)
  head=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["head"])' "$dir/request.json")
  base=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["base"] or "-")' "$dir/request.json")
  git -C "$clone" worktree add --detach "$work/checkout" "$head" > /dev/null 2>&1
  if [ -d "$dir/local" ]; then
    (cd "$dir/local/append" 2>/dev/null && find . -type f) | while read -r file; do
      cat "$dir/local/append/$file" >> "$work/checkout/${file%.fixture}"
    done
    (cd "$dir/local/untracked" 2>/dev/null && find . -type f) | while read -r file; do
      cp "$dir/local/untracked/$file" "$work/checkout/${file%.fixture}"
    done
  fi
  mkdir -p "$work/temp/velnor/request"
  cp "$dir/request.json" "$work/temp/velnor/request/plan-v1-request.json"
  rm -f "$dir/expected_outputs"
  (cd "$work/checkout" && env -i PATH="$mise_dir:/usr/bin:/bin" HOME="$HOME" \
    RUNNER_TEMP="$work/temp" GITHUB_OUTPUT="$work/outputs" GITHUB_WORKSPACE="$work/checkout" \
    GITHUB_RUN_ID=44491127982 GITHUB_RUN_ATTEMPT=1 VELNOR_INTERNAL_OP=plan-v1 \
    VELNOR_REQUEST_FILE="$work/temp/velnor/request/plan-v1-request.json" \
    "$engine" 2> "$work/stderr") || { cat "$work/stderr" >&2; echo "$name: the op refused" >&2; exit 1; }
  grep -v '^matrix=\|^qualification_cache_directives=' "$work/outputs" > "$dir/expected_outputs"
  rm -rf "$dir/snapshot"
  edited=$(cd "$dir/local/append" 2>/dev/null && find . -type f | sed 's|^\./||; s|\.fixture$||' || true)
  # shellcheck disable=SC2086
  python3 "$here/snapshot.py" "$clone" "$base" "$head" "$dir/snapshot" $edited
  git -C "$clone" worktree remove --force "$work/checkout"
  rm -rf "$work"
  echo "$name: $(cat "$dir/expected_outputs" | grep not_affected_jobs | cut -c1-200)"
done
