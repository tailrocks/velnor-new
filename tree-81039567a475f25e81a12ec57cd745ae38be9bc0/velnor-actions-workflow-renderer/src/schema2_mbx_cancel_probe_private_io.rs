//! Fixed, bounded filesystem helpers for cancellation evidence.

pub(super) const PRIVATE_IO_HELPERS: &str = r#"
private_stat() {
  local field="$1" path="$2" format
  case "$field" in
    owner) format=%u ;;
    links) format=%h ;;
    size) format=%s ;;
    mode) format=%a ;;
    *) return 1 ;;
  esac
  if stat -c "$format" -- "$path" >/dev/null 2>&1; then
    stat -c "$format" -- "$path" 2>/dev/null
  else
    case "$field" in
      owner) format=%u ;;
      links) format=%l ;;
      size) format=%z ;;
      mode) format=%Lp ;;
    esac
    stat -f "$format" "$path" 2>/dev/null
  fi
}

private_canonical_directory() {
  (cd -P -- "$1" 2>/dev/null && pwd -P)
}

private_runner_temp_valid() {
  local path="${RUNNER_TEMP:-}" canonical owner mode uid
  [[ "$path" = /* ]] || return 1
  [ -d "$path" ] && [ ! -L "$path" ] || return 1
  canonical="$(private_canonical_directory "$path")" || return 1
  [ "$canonical" = "$path" ] || return 1
  uid="$(id -u)" || return 1
  owner="$(private_stat owner "$path")" || return 1
  mode="$(private_stat mode "$path")" || return 1
  [ "$owner" = "$uid" ] && [[ "$mode" =~ ^[0-7]{3,4}$ ]] \
    && (( (8#$mode & 0022) == 0 ))
}

private_root_allowed() {
  case "$1" in
    "$RUNNER_TEMP/mbx-cancel-controller"|"$RUNNER_TEMP/mbx-cancel-victim"|"$RUNNER_TEMP/mbx-cancel-observer") return 0 ;;
    "$RUNNER_TEMP/mbx-cancel-restore-parent"|"$RUNNER_TEMP/mbx-cancel-restore-save") return 0 ;;
    *) return 1 ;;
  esac
}

private_root_open() {
  local path="$1" canonical owner mode uid
  private_runner_temp_valid || return 1
  private_root_allowed "$path" || return 1
  [ -d "$path" ] && [ ! -L "$path" ] || return 1
  canonical="$(private_canonical_directory "$path")" || return 1
  [ "$canonical" = "$path" ] || return 1
  uid="$(id -u)" || return 1
  owner="$(private_stat owner "$path")" || return 1
  mode="$(private_stat mode "$path")" || return 1
  [ "$owner" = "$uid" ] && [ "$mode" = 700 ]
}

private_storage_open() {
  local path="$1" parent
  if private_root_open "$path"; then return 0; fi
  case "$path" in
    "$RUNNER_TEMP/mbx-cancel-victim/source") parent="$RUNNER_TEMP/mbx-cancel-victim" ;;
    "$RUNNER_TEMP/mbx-cancel-observer/controller-receipt"|\
    "$RUNNER_TEMP/mbx-cancel-observer/observer") parent="$RUNNER_TEMP/mbx-cancel-observer" ;;
    *) return 1 ;;
  esac
  private_child_open "$parent" "$path"
}

private_root_create() {
  local path="$1"
  private_runner_temp_valid || return 1
  private_root_allowed "$path" || return 1
  [ ! -e "$path" ] && [ ! -L "$path" ] || return 1
  mkdir -m 700 -- "$path" || return 1
  private_root_open "$path"
}

private_root_remove() {
  local path="$1"
  private_root_open "$path" && rm -rf -- "$path"
}

private_child_allowed() {
  case "$1:$2" in
    "$RUNNER_TEMP/mbx-cancel-victim:$RUNNER_TEMP/mbx-cancel-victim/source"|\
    "$RUNNER_TEMP/mbx-cancel-observer:$RUNNER_TEMP/mbx-cancel-observer/controller-receipt"|\
    "$RUNNER_TEMP/mbx-cancel-observer:$RUNNER_TEMP/mbx-cancel-observer/observer") return 0 ;;
    *) return 1 ;;
  esac
}

private_child_open() {
  local root="$1" path="$2" canonical owner mode uid
  private_root_open "$root" && private_child_allowed "$root" "$path" || return 1
  [ -d "$path" ] && [ ! -L "$path" ] || return 1
  canonical="$(private_canonical_directory "$path")" || return 1
  [ "$canonical" = "$path" ] || return 1
  uid="$(id -u)" || return 1
  owner="$(private_stat owner "$path")" || return 1
  mode="$(private_stat mode "$path")" || return 1
  [ "$owner" = "$uid" ] && [ "$mode" = 700 ]
}

private_child_create() {
  local root="$1" path="$2"
  private_root_open "$root" && private_child_allowed "$root" "$path" || return 1
  [ ! -e "$path" ] && [ ! -L "$path" ] || return 1
  mkdir -m 700 -- "$path" || return 1
  private_child_open "$root" "$path"
}

private_path_allowed() {
  local root="$1" path="$2" relative rest part
  case "$path" in "$root"/*) relative="${path#"$root"/}" ;; *) return 1 ;; esac
  [[ "$relative" =~ ^[A-Za-z0-9][A-Za-z0-9._/-]{0,254}$ ]] || return 1
  rest="$relative"
  while :; do
    part="${rest%%/*}"
    [[ "$part" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || return 1
    [ "$part" != . ] && [ "$part" != .. ] || return 1
    [[ "$rest" = */* ]] || break
    rest="${rest#*/}"
    [ -n "$rest" ] || return 1
  done
}

private_directory_open() {
  local root="$1" path="$2" relative rest part current canonical owner mode uid
  private_storage_open "$root" || return 1
  [ "$path" = "$root" ] && return 0
  private_path_allowed "$root" "$path" || return 1
  relative="${path#"$root"/}"
  rest="$relative"
  current="$root"
  uid="$(id -u)" || return 1
  while :; do
    part="${rest%%/*}"
    current="$current/$part"
    [ -d "$current" ] && [ ! -L "$current" ] || return 1
    canonical="$(private_canonical_directory "$current")" || return 1
    [ "$canonical" = "$current" ] || return 1
    owner="$(private_stat owner "$current")" || return 1
    mode="$(private_stat mode "$current")" || return 1
    [ "$owner" = "$uid" ] && [ "$mode" = 700 ] || return 1
    [[ "$rest" = */* ]] || break
    rest="${rest#*/}"
  done
}

private_path_parent_open() {
  local root="$1" path="$2" parent
  [[ "$path" = */* ]] || return 1
  parent="${path%/*}"
  private_directory_open "$root" "$parent"
}

private_file_valid() {
  local root="$1" path="$2" max_bytes="$3" canonical owner links bytes uid
  private_storage_open "$root" && private_path_allowed "$root" "$path" \
    && private_path_parent_open "$root" "$path" || return 1
  [ -f "$path" ] && [ ! -L "$path" ] || return 1
  canonical="$(private_canonical_directory "${path%/*}")" || return 1
  [ "$canonical/$(basename -- "$path")" = "$path" ] || return 1
  owner="$(private_stat owner "$path")" || return 1
  links="$(private_stat links "$path")" || return 1
  bytes="$(private_stat size "$path")" || return 1
  uid="$(id -u)" || return 1
  [[ "$owner" =~ ^[0-9]+$ ]] && [ "$owner" = "$uid" ] && [ "$links" = 1 ] \
    && [[ "$bytes" =~ ^[0-9]+$ ]] && [ "$bytes" -le "$max_bytes" ]
}

private_remove_file() {
  local root="$1" path="$2" max_bytes="$3"
  if [ -e "$path" ] || [ -L "$path" ]; then
    private_file_valid "$root" "$path" "$max_bytes" || return 1
    rm -- "$path"
  fi
}

private_external_file_valid() {
  local path="$1" max_bytes="$2" canonical owner links bytes uid
  private_runner_temp_valid || return 1
  case "$path" in "$RUNNER_TEMP"/*) ;; *) return 1 ;; esac
  [ -f "$path" ] && [ ! -L "$path" ] || return 1
  canonical="$(private_canonical_directory "${path%/*}")" || return 1
  [ "$canonical/$(basename -- "$path")" = "$path" ] || return 1
  owner="$(private_stat owner "$path")" || return 1
  links="$(private_stat links "$path")" || return 1
  bytes="$(private_stat size "$path")" || return 1
  uid="$(id -u)" || return 1
  [[ "$owner" =~ ^[0-9]+$ ]] && [ "$owner" = "$uid" ] && [ "$links" = 1 ] \
    && [[ "$bytes" =~ ^[0-9]+$ ]] && [ "$bytes" -gt 0 ] && [ "$bytes" -le "$max_bytes" ]
}

private_json_valid() {
  local root="$1" path="$2" max_bytes="$3"
  private_file_valid "$root" "$path" "$max_bytes" \
    && stock_restore_single_object "$path" "$max_bytes" >/dev/null 2>&1
}

private_event_valid() {
  local path="${GITHUB_EVENT_PATH:-}"
  [ -n "$path" ] && private_external_file_valid "$path" 65536 \
    && stock_restore_single_object "$path" 65536 >/dev/null 2>&1
}

private_capture() {
  local root="$1" path="$2" max_bytes="$3" size result
  shift 3
  private_storage_open "$root" && private_path_allowed "$root" "$path" \
    && private_path_parent_open "$root" "$path" || return 1
  if [ -e "$path" ] || [ -L "$path" ]; then
    private_file_valid "$root" "$path" "$max_bytes" || return 1
    rm -- "$path" || return 1
  fi
  if (umask 077; set -C; "$@" | head -c "$((max_bytes + 1))" > "$path"); then
    result=0
  else
    result=$?
  fi
  size="$(private_stat size "$path")" || return 1
  if [[ "$size" =~ ^[0-9]+$ ]] && [ "$size" -le "$max_bytes" ] \
    && [ "$result" = 0 ] && private_file_valid "$root" "$path" "$max_bytes"; then
    return 0
  fi
  if private_file_valid "$root" "$path" "$((max_bytes + 1))"; then rm -- "$path"; fi
  return 1
}

private_gh_json() {
  local root="$1" path="$2"
  shift 2
  private_capture "$root" "$path" 2097152 gh_api "$@" \
    && private_json_valid "$root" "$path" 2097152
}

private_list_complete() {
  local path="$1" field="$2"
  jq -e --arg field "$field" '
    type == "object" and (.total_count | type == "number" and . >= 0 and . == floor)
    and ((.[$field] | type) == "array") and .total_count == (.[$field] | length)
    and all(.[$field][]; type == "object")
  ' "$path" >/dev/null 2>&1
}
"#;

pub(super) const OBSERVER_INIT: &str = r#"set -euo pipefail
umask 077
root="$RUNNER_TEMP/mbx-cancel-observer"
private_root_create "$root"
private_child_create "$root" "$root/controller-receipt"
private_child_create "$root" "$root/observer"
private_root_create "$RUNNER_TEMP/mbx-cancel-restore-parent"
private_root_create "$RUNNER_TEMP/mbx-cancel-restore-save"
"#;
