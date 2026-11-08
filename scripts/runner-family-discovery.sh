#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

family_require_current_main() {
  local ref
  ref="$(gh api "repos/$GITHUB_REPOSITORY/git/ref/heads/main")" || return 1
  jq -e --arg sha "$GITHUB_SHA" '
    .ref == "refs/heads/main" and .object.type == "commit" and
    (.object.sha | type == "string" and test("^[0-9a-f]{40}$")) and .object.sha == $sha
  ' <<<"$ref" >/dev/null
}

family_run() {
  local workflow="$1" deadline="$2" require_current_main="${3:-false}" runs run state
  [[ "$require_current_main" == true || "$require_current_main" == false ]] || return 1
  while (( SECONDS < deadline )); do
    if [[ "$require_current_main" == true ]]; then family_require_current_main || return 1; fi
    runs="$(gh api --paginate --slurp "repos/$GITHUB_REPOSITORY/actions/workflows/$workflow/runs?branch=main&event=push&head_sha=$GITHUB_SHA&per_page=100")"
    run="$(jq -c --arg path ".github/workflows/$workflow" --arg sha "$GITHUB_SHA" --arg repository "$GITHUB_REPOSITORY" '
      if length == 0 or (.[0].total_count | type) != "number" or .[0].total_count >= 1000 then error("incomplete workflow runs") else .[0].total_count as $total
      | if (all(.[]; .total_count == $total) and ([.[].workflow_runs[]] | length) == $total) then . else error("incomplete workflow runs") end end
      | [.[].workflow_runs[] | select(.path == $path and .event == "push" and .head_branch == "main" and .head_sha == $sha and .head_repository.full_name == $repository and (.id | type) == "number" and (.run_number | type) == "number" and (.run_attempt | type) == "number")]
      | sort_by([.run_number, .id])
      | if length == 0 then null else .[-1] as $latest | if ([.[] | select(.run_number == $latest.run_number)] | length) == 1 then $latest else error("ambiguous workflow run") end end
    ' <<<"$runs")" || return 1
    if [[ "$run" != null ]]; then
      state="$(jq -r '.status' <<<"$run")"
      if [[ "$state" == completed ]]; then
        [[ "$(jq -r '.conclusion' <<<"$run")" == success ]] || return 1
        if [[ "$require_current_main" == true ]]; then family_require_current_main || return 1; fi
        printf '%s\n' "$run"
        return 0
      fi
    fi
    sleep 5
  done
  return 1
}

family_artifact() {
  local run_id="$1" name="$2" artifacts
  artifacts="$(gh api --paginate --slurp "repos/$GITHUB_REPOSITORY/actions/runs/$run_id/artifacts?per_page=100")"
  jq -er --arg name "$name" --arg sha "$GITHUB_SHA" --argjson run "$run_id" '
    if length == 0 or (.[0].total_count | type) != "number" then error("incomplete artifacts") else .[0].total_count as $total
    | if (all(.[]; .total_count == $total) and ([.[].artifacts[]] | length) == $total) then . else error("incomplete artifacts") end end
    | [.[].artifacts[] | select(.name == $name and .expired == false and .size_in_bytes > 0 and .workflow_run.id == $run and .workflow_run.head_sha == $sha and .workflow_run.head_branch == "main")]
    | if length == 1 and (.[0].id | type) == "number" and .[0].id > 0 and (.[0].id | floor) == .[0].id then .[0].id else error("missing or ambiguous artifact") end
  ' <<<"$artifacts"
}
