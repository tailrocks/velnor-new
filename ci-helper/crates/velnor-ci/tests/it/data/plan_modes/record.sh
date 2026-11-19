#!/bin/sh
# Records what the legacy plan-v1 op does for each case here.
# Usage: record.sh <velnor-actions binary built from this repository> <clone of tailrocks/velnor-new> <dir holding the pinned mise>
# A case holds request.json, the plan request the op reads (what write-request writes, with the
# lane inventory and named check lanes of ci.yml at the case's head). The op plans a checkout of that
# head, so each case runs in a detached worktree of the clone, and the op needs rust installed by that mise.
# It writes expected_outputs: the step outputs the op wrote, minus `matrix` and
# `qualification_cache_directives`, which carry the task digests the helper does not compute.
# On a refusal it writes expected_error: the code after "internal request failed: internal: ".
# A case holding legacy_refusal is not recorded: the op plans it past the qualification checks and
# then refuses it (see that file), so there is no legacy success to compare with.
# Optional further arguments name the cases to record; the default is all of them.
set -eu
engine=$1
clone=$2
mise_dir=$3
shift 3
here=$(cd "$(dirname "$0")" && pwd)
for dir in "$here"/*/; do
  name=$(basename "$dir")
  [ -f "$dir/legacy_refusal" ] && continue
  if [ "$#" -gt 0 ]; then case " $* " in *" $name "*) ;; *) continue ;; esac; fi
  work=$(mktemp -d)
  head=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["head"])' "$dir/request.json")
  git -C "$clone" worktree add --detach "$work/checkout" "$head" > /dev/null 2>&1
  mkdir -p "$work/temp/velnor/request"
  cp "$dir/request.json" "$work/temp/velnor/request/plan-v1-request.json"
  rm -f "$dir/expected_outputs" "$dir/expected_error"
  if (cd "$work/checkout" && env -i PATH="$mise_dir:/usr/bin:/bin" HOME="$HOME" \
    RUNNER_TEMP="$work/temp" GITHUB_OUTPUT="$work/outputs" GITHUB_WORKSPACE="$work/checkout" \
    GITHUB_RUN_ID=44491127982 GITHUB_RUN_ATTEMPT=1 VELNOR_INTERNAL_OP=plan-v1 \
    VELNOR_REQUEST_FILE="$work/temp/velnor/request/plan-v1-request.json" \
    "$engine" 2> "$work/stderr"); then
    grep -v '^matrix=\|^qualification_cache_directives=' "$work/outputs" > "$dir/expected_outputs"
  else
    sed -n 's/^.*internal request failed: internal: //p' "$work/stderr" | tr -d '\n' > "$dir/expected_error"
    [ -s "$dir/expected_error" ] || { cat "$work/stderr" >&2; echo "$name: no refusal code" >&2; exit 1; }
  fi
  git -C "$clone" worktree remove --force "$work/checkout"
  rm -rf "$work"
  echo "$name: $(ls "$dir" | tr '\n' ' ')"
done
