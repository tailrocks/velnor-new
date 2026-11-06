# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

: "${base:?}" "${repo:?}" "${sha:?}"

list_runs() {
  workflow=$1
  : > "$base/runs.jsonl"
  page=1
  while [ "$page" -le 10 ]; do
    gh api "repos/$repo/actions/workflows/$workflow/runs?head_sha=$sha&event=push&per_page=100&page=$page" > "$base/page.json"
    count=$(jq -er '.workflow_runs | length | select(. <= 100)' "$base/page.json")
    jq -c --arg sha "$sha" '.workflow_runs[] | select(.head_sha == $sha and .event == "push" and .head_branch == "main" and .head_repository.id == 1390620900)' "$base/page.json" >> "$base/runs.jsonl"
    [ "$count" -lt 100 ] && break
    page=$((page + 1))
  done
  [ "$page" -le 10 ]
  jq -s -er 'sort_by(.id) | last | select(.status == "completed" and .conclusion == "success") | [.id, .run_attempt] | @tsv' "$base/runs.jsonl"
}
find_artifact() {
  run=$1
  name=$2
  : > "$base/artifacts.jsonl"
  page=1
  while [ "$page" -le 10 ]; do
    gh api "repos/$repo/actions/runs/$run/artifacts?per_page=100&page=$page" > "$base/page.json"
    count=$(jq -er '.artifacts | length | select(. <= 100)' "$base/page.json")
    jq -c --arg name "$name" '.artifacts[] | select(.name == $name and .expired == false)' "$base/page.json" >> "$base/artifacts.jsonl"
    [ "$count" -lt 100 ] && break
    page=$((page + 1))
  done
  [ "$page" -le 10 ]
  jq -s -er 'if length == 1 and .[0].id > 0 then .[0].id else error("artifact identity") end' "$base/artifacts.jsonl"
}
validate_required() {
  run=$1
  attempt=$2
  : > "$base/jobs.jsonl"
  page=1
  while [ "$page" -le 10 ]; do
    gh api "repos/$repo/actions/runs/$run/attempts/$attempt/jobs?per_page=100&page=$page" > "$base/page.json"
    count=$(jq -er '.jobs | length | select(. <= 100)' "$base/page.json")
    jq -c '.jobs[] | select(.name == "Required")' "$base/page.json" >> "$base/jobs.jsonl"
    [ "$count" -lt 100 ] && break
    page=$((page + 1))
  done
  [ "$page" -le 10 ]
  jq -s -e 'length == 1 and .[0].status == "completed" and .[0].conclusion == "success"' "$base/jobs.jsonl" > /dev/null
}
