escape_path() {
  local value="$1"
  value="${value//%/%25}"
  value="${value//$'\t'/%09}"
  value="${value//$'\n'/%0A}"
  value="${value//$'\r'/%0D}"
  printf '%s' "$value"
}

evidence_marker() {
  printf 'mbx-cache-evidence-v1\t%s\t%s\t%s\n' \
    "${GITHUB_RUN_ID-}" "${GITHUB_RUN_ATTEMPT-}" "${MBX_QUALIFICATION_JOB_ID-}"
}

capture_walk_ancestry() {
  local root="$1" current=/ segment
  local -a components=()
  walk_ancestor_paths=(/)
  walk_ancestor_ids=("$(stat -c '%d:%i' /)")
  IFS='/' read -r -a components <<< "${root#/}"
  for segment in "${components[@]}"; do
    [[ -n "$segment" && "$segment" != . && "$segment" != .. ]] || return 1
    current="${current%/}/$segment"
    [[ -d "$current" && ! -L "$current" && "$(realpath -e -- "$current")" == "$current" ]] || return 1
    walk_ancestor_paths+=("$current")
    walk_ancestor_ids+=("$(stat -c '%d:%i' -- "$current")")
  done
  [[ "$current" == "$root" ]]
}

validate_walk_ancestry() {
  local index path
  for index in "${!walk_ancestor_paths[@]}"; do
    path="${walk_ancestor_paths[$index]}"
    [[ -d "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" ]] || return 1
    [[ "$(stat -c '%d:%i' -- "$path")" == "${walk_ancestor_ids[$index]}" ]] || return 1
  done
}

validate_evidence() {
  local canonical marker mode_bits owner path_stat
  [[ -d "$evidence" && ! -L "$evidence" ]] || return 1
  canonical="$(realpath -e -- "$evidence")" || return 1
  [[ "$canonical" == "$evidence" ]] || return 1
  owner="$(stat -c '%u:%g' -- "$evidence")" || return 1
  [[ "$owner" == "$(id -u):$(id -g)" ]] || return 1
  capture_walk_ancestry "$evidence" || return 1
  validate_walk_ancestry || return 1
  [[ -f "$evidence/private.marker" && ! -L "$evidence/private.marker" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/private.marker")" == "1:600:$(id -u):$(id -g)" ]] || return 1
  marker="$(cat -- "$evidence/private.marker")"
  [[ "$marker" == "$(evidence_marker)" ]] || return 1
  [[ -f "$evidence/private.identity" && ! -L "$evidence/private.identity" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/private.identity")" == "1:600:$(id -u):$(id -g)" ]] || return 1
  [[ "$(cat -- "$evidence/private.identity")" == "$(stat -c '%d:%i:%u:%g' -- "$evidence")" ]] || return 1
  mode_bits="$(stat -c '%a' -- "$evidence")"
  [[ "$mode_bits" == 700 ]] || return 1
  [[ -f "$evidence/root-registry.tsv" && ! -L "$evidence/root-registry.tsv" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/root-registry.tsv")" == "1:600:$(id -u):$(id -g)" ]] || return 1
  [[ -f "$evidence/sampler.sh" && ! -L "$evidence/sampler.sh" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/sampler.sh")" == "1:700:$(id -u):$(id -g)" ]] || return 1
  [[ -f "$evidence/path-validation.sh" && ! -L "$evidence/path-validation.sh" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/path-validation.sh")" == "1:700:$(id -u):$(id -g)" ]] || return 1
  [[ "$MBX_QUALIFICATION_PHASE_FILE" == "$evidence/phases.tsv" ]] || return 1
  [[ -f "$evidence/phases.tsv" && ! -L "$evidence/phases.tsv" ]] || return 1
  path_stat="$(stat -c '%h:%a:%u:%g' -- "$evidence/phases.tsv")"
  [[ "$path_stat" == "1:600:$(id -u):$(id -g)" ]]
}
