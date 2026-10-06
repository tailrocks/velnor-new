#!/usr/bin/env bash
set -u -o pipefail
umask 077
PATH=/usr/bin:/bin
export PATH
SCRATCH=""
KEEP_SCRATCH=0
LOCK_SNAPSHOT=""
LOCK_PRESENT=0
CACHE_USABLE=0
CACHE_DENIED=0
NATIVE_CACHE=""
EXPECTED_KEY=""
cleanup() {
  local status=$?
  if test "$KEEP_SCRATCH" -eq 0 && test -n "${SCRATCH:-}"; then
    rm -rf -- "$SCRATCH"
  fi
  return "$status"
}
trap cleanup EXIT HUP INT TERM
path_ok() {
  local path="$1" cursor
  case "$path" in
    /*) ;;
    *) return 1 ;;
  esac
  case "$path" in
    *$'\n'*|*$'\r'*|*'//'*) return 1 ;;
    /) ;;
    */) return 1 ;;
    */./*|*/../*|*/.|*/..) return 1 ;;
  esac
  cursor="$path"
  while test "$cursor" != /; do
    test ! -L "$cursor" || return 1
    cursor="${cursor%/*}"
    test -n "$cursor" || cursor=/
  done
}
relative_root_ok() {
  local root="$1" part cursor=.
  case "$root" in
    '') return 0 ;;
    /*|-*|*$'\n'*|*$'\r'*) return 1 ;;
  esac
  case "/$root/" in
    *//*|*/../*) return 1 ;;
  esac
  IFS=/ read -r -a parts <<< "$root"
  for part in "${parts[@]}"; do
    test -n "$part" && test "$part" != . && test "$part" != .. || return 1
    cursor="$cursor/$part"
    test "${2-}" != 1 || test ! -L "$cursor" || return 1
  done
}
link_count() {
  local path="$1" os
  os="$(/usr/bin/uname -s)" || return 1
  case "$os" in
    Darwin) /usr/bin/stat -f '%l' "$path" ;;
    Linux) /usr/bin/stat -c '%h' "$path" ;;
    *) return 1 ;;
  esac
}
regular_file() {
  local path="$1"
  test -f "$path" && test ! -L "$path"
}
single_link_file() {
  local path="$1" links
  regular_file "$path" || return 1
  links="$(link_count "$path")" || return 1
  test "$links" = 1
}
marker_matches() {
  local marker="$1"
  single_link_file "$marker" && cmp -s "$EXPECTED_KEY" "$marker"
}
write_marker() {
  local dir="$1" marker="$1/.velnor-root-key" temporary
  path_ok "$marker" && test ! -e "$marker" && test ! -L "$marker" || return 1
  temporary="$(mktemp "$dir/.velnor-root-key.XXXXXX")" || return 1
  cat "$EXPECTED_KEY" > "$temporary" || { rm -f -- "$temporary"; return 1; }
  if ! chmod 0600 "$temporary" || ! single_link_file "$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  ln "$temporary" "$marker" || { rm -f -- "$temporary"; return 1; }
  rm "$temporary" || return 1
  marker_matches "$marker"
}
owned_dir() {
  local dir="$1" marker="$1/.velnor-root-key" entry
  path_ok "$dir" && test -d "$dir" && test ! -L "$dir" || return 1
  if test -e "$marker" || test -L "$marker"; then
    marker_matches "$marker"
    return $?
  fi
  for entry in "$dir"/* "$dir"/.[!.]* "$dir"/..?*; do
    test -e "$entry" || test -L "$entry" || continue
    return 1
  done
  write_marker "$dir"
}
ensure_dir() {
  local path="$1"
  path_ok "$path" || return 1
  if test -e "$path" || test -L "$path"; then
    test -d "$path" && test ! -L "$path" || return 1
  else
    mkdir -p -- "$path" || return 1
  fi
  path_ok "$path"
}
version_ok() {
  [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][a-zA-Z0-9.-]+)?$ ]]
}
root_key_ok() {
  local key="$1" hex
  case "$key" in
    dir-*) ;;
    *) return 1 ;;
  esac
  hex="${key#dir-}"
  case "$hex" in
    *[!a-f0-9]*) return 1 ;;
  esac
  test $(( ${#hex} % 2 )) -eq 0
}
paths_disjoint() {
  local left="$1" right="$2"
  test "$left" != "$right" || return 1
  case "$left/" in
    "$right/"*) return 1 ;;
  esac
  case "$right/" in
    "$left/"*) return 1 ;;
  esac
}
write_config() {
  local path="$1" cache="$2" parent temporary
  parent="${path%/*}"
  test "$parent" != "$path" || parent=/
  ensure_dir "$parent" || return 1
  if test -e "$path" || test -L "$path"; then
    single_link_file "$path" || return 1
  fi
  temporary="$(mktemp "$parent/.tofu-cli.XXXXXX")" || return 1
  path_ok "$temporary" && single_link_file "$temporary" || return 1
  if test -n "$cache"; then
    printf 'plugin_cache_dir = "%s"\ndisable_checkpoint = true\nprovider_installation {\n  direct {}\n}\n' \
      "$cache" > "$temporary" || return 1
  else
    printf 'disable_checkpoint = true\nprovider_installation {\n  direct {}\n}\n' \
      > "$temporary" || return 1
  fi
  chmod 0600 "$temporary" || return 1
  single_link_file "$temporary" || return 1
  path_ok "$path" || return 1
  mv -f -- "$temporary" "$path" || return 1
  single_link_file "$path"
}
classifier() {
  /usr/bin/python3 -I -S - "$1" "$2" <<'PY'
import json,pathlib,sys
PREFIX = "existing cached package at "; SUFFIX = " does not match the content of the downloaded package; does it contain local modifications?"
def object_pairs(pairs):
    value = {}
    for key,item in pairs:
        if key in value: raise ValueError("duplicate JSON key")
        value[key] = item
    return value
def reject_constant(value): raise ValueError("non-standard JSON constant: " + value)
def mismatch(detail, cache):
    head, sep, tail = detail.partition(PREFIX)
    if not detail.startswith("Error while installing ") or not sep or not head.endswith(": ") or not tail.endswith(SUFFIX): return False
    package = head[len("Error while installing "):-2]; path = tail[:-len(SUFFIX)]
    if " v" not in package or not path.startswith(cache + "/") or path.endswith("/"): return False
    if any(token in path for token in ("\n", "\r", "//", "/./", "/../")): return False
    source, version = package.rsplit(" v", 1); return bool("/" in source and not any(c.isspace() for c in source) and version and version[0].isdigit() and all(c.isalnum() or c in ".+-" for c in version) and PREFIX not in path and SUFFIX not in path)
try:
    lines = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
    if not lines:
        raise ValueError("empty JSONL")
    errors = []
    for line in lines:
        if not line:
            raise ValueError("blank JSONL record")
        event = json.loads(line, object_pairs_hook=object_pairs,
                           parse_constant=reject_constant)
        if not isinstance(event, dict):
            raise ValueError("non-object JSONL record")
        level = event.get("@level")
        kind = event.get("type")
        diagnostic = event.get("diagnostic")
        if level == "error" and kind != "diagnostic":
            raise ValueError("unknown error-level record")
        if kind != "diagnostic":
            continue
        if not isinstance(diagnostic, dict):
            raise ValueError("malformed diagnostic")
        severity = diagnostic.get("severity")
        if level == "error" and severity != "error":
            raise ValueError("error-level diagnostic has non-error severity")
        if severity == "error":
            if level != "error":
                raise ValueError("error diagnostic has non-error level")
            summary = diagnostic.get("summary")
            detail = diagnostic.get("detail")
            if not isinstance(summary, str) or not isinstance(detail, str):
                raise ValueError("malformed error diagnostic")
            errors.append((summary, detail))
    if not errors:
        raise SystemExit(1)
    cache = sys.argv[2]
    if all(summary == "Failed to install provider" and mismatch(detail, cache) for summary, detail in errors):
        raise SystemExit(0)
    raise SystemExit(1)
except SystemExit: raise
except Exception:
    raise SystemExit(2)
PY
}
native_init() {
  local tofu="$1" root="$2" data="$3" home="$4" config="$5" cache="$6"
  local output="$7" error="$8" status
  local -a environment
  environment=(PATH=/usr/bin:/bin HOME="$home" TF_DATA_DIR="$data"
    TF_CLI_CONFIG_FILE="$config" TF_IN_AUTOMATION=1 TF_INPUT=0 NO_COLOR=1)
  test -n "$cache" && environment+=(TF_PLUGIN_CACHE_DIR="$cache")
  if test -n "$root"; then
    /usr/bin/env -i "${environment[@]}" "$tofu" -chdir "$root" init -json \
      -backend=false -input=false -lockfile=readonly -no-color > "$output" 2> "$error"
  else
    /usr/bin/env -i "${environment[@]}" "$tofu" init -json -backend=false \
      -input=false -lockfile=readonly -no-color > "$output" 2> "$error"
  fi
  status=$?
  return "$status"
}
relay() { cat "$1"; cat "$2" >&2; }
lock_state_unchanged() {
  local lock="$1" snapshot="$2" present="$3"
  if test "$present" -eq 1; then
    regular_file "$lock" && cmp -s "$snapshot" "$lock"
  else
    test ! -e "$lock" && test ! -L "$lock"
  fi
}
capture_lock() {
  local lock="$1"
  LOCK_PRESENT=0
  LOCK_SNAPSHOT=""
  if test -e "$lock" || test -L "$lock"; then
    regular_file "$lock" || return 1
    LOCK_PRESENT=1
    LOCK_SNAPSHOT="$SCRATCH/lock.snapshot"
    cat "$lock" > "$LOCK_SNAPSHOT" || return 1
    single_link_file "$LOCK_SNAPSHOT" && cmp -s "$LOCK_SNAPSHOT" "$lock"
  fi
}
cache_transport() {
  local cache="$1" marker listing bad entry entries links
  CACHE_USABLE=0
  CACHE_DENIED=0
  NATIVE_CACHE=""
  path_ok "$cache" && test -d "$cache" && test ! -L "$cache" || return 1
  marker="$cache/.velnor-root-key"
  listing="$SCRATCH/cache.list"
  bad="$SCRATCH/cache.bad"
  /usr/bin/find "$cache" -print > "$listing" || return 1
  /usr/bin/find "$cache" \( -type l -o \( ! -type d -a ! -type f \) \
    -o \( -type f -links +1 \) \) -print > "$bad" || return 1
  entries=0
  while IFS= read -r entry; do
    test "$entry" = "$cache" && continue
    entries=1
  done < "$listing"
  if test "$entries" -eq 0; then write_marker "$cache" || return 1; CACHE_USABLE=1; NATIVE_CACHE="$cache"; return 0; fi
  if ! test -f "$marker" || test -L "$marker"; then
    CACHE_DENIED=1
    return 0
  fi
  links="$(link_count "$marker")" || return 1
  if test "$links" != 1; then
    CACHE_DENIED=1
    return 0
  fi
  if ! path_ok "$marker"; then
    CACHE_DENIED=1
    return 0
  fi
  if ! cmp -s "$EXPECTED_KEY" "$marker"; then
    CACHE_DENIED=1
    return 0
  fi
  if test -s "$bad"; then
    CACHE_DENIED=1
    return 0
  fi
  CACHE_USABLE=1
  NATIVE_CACHE="$cache"
}
quarantine_data() {
  local data="$1" old="$SCRATCH/original-data"
  path_ok "$data" && test -d "$data" && test ! -L "$data" || return 1
  marker_matches "$data/.velnor-root-key" || return 1
  path_ok "$old" || return 1
  test ! -e "$old" && test ! -L "$old" || return 1
  mv -- "$data" "$old" || return 1
  KEEP_SCRATCH=1
  test ! -e "$data" && test ! -L "$data" || return 1
  mkdir -m 700 -- "$data" || return 1
  path_ok "$data" && test -d "$data" && test ! -L "$data" || return 1
  write_marker "$data" || return 1
  KEEP_SCRATCH=1
}
prepare_data() { test "$CACHE_DENIED" -eq 0 || quarantine_data "$1"; }
main() {
  local root_arg="${1-}" root pin="${2-}" root_key="${3-}" binary_rel="${4-}" tofu data home config cache lock
  local parent retry_home retry_config status classify_status
  local first_output first_error retry_output retry_error
  test "$#" -eq 4 && test -n "$root_arg" && test -n "$binary_rel" || return 1
  test "$root_arg" = . && root="" || root="$root_arg"
  relative_root_ok "$root" 1 && version_ok "$pin" || return 1
  relative_root_ok "$binary_rel" || return 1
  root_key_ok "$root_key" || return 1
  for name in HOME TF_DATA_DIR TF_CLI_CONFIG_FILE TF_PLUGIN_CACHE_DIR MISE_DATA_DIR; do
    test -n "${!name-}" || return 1
    path_ok "${!name}" || return 1
  done
  home="$HOME"
  data="$TF_DATA_DIR"
  config="$TF_CLI_CONFIG_FILE"
  cache="$TF_PLUGIN_CACHE_DIR"
  paths_disjoint "$home" "$data" && paths_disjoint "$home" "$cache" \
    && paths_disjoint "$home" "$MISE_DATA_DIR" && paths_disjoint "$data" "$cache" \
    && paths_disjoint "$data" "$MISE_DATA_DIR" && paths_disjoint "$cache" "$MISE_DATA_DIR" \
    || return 1
  case "$config" in
    "$data"|"$data/"*) return 1 ;;
  esac
  case "$config" in
    "$cache"|"$cache/"*) return 1 ;;
  esac
  ensure_dir "$home" && ensure_dir "$data" && ensure_dir "$cache" || return 1
  parent="${data%/*}"
  test "$parent" != "$data" || parent=/
  ensure_dir "$parent" || return 1
  SCRATCH="$(mktemp -d "$parent/.tofu-cached-init.XXXXXX")" || return 1
  path_ok "$SCRATCH" || return 1
  EXPECTED_KEY="$SCRATCH/expected-root-key"
  printf '%s' "$root_key" > "$EXPECTED_KEY" || return 1
  single_link_file "$EXPECTED_KEY" || return 1
  owned_dir "$home" && owned_dir "$data" || return 1
  cache_transport "$cache" || return 1
  test -d "$MISE_DATA_DIR" && test ! -L "$MISE_DATA_DIR" || return 1
  tofu="$MISE_DATA_DIR/$binary_rel"
  path_ok "$tofu" && regular_file "$tofu" && test -x "$tofu" || return 1
  if test -n "$root"; then
    lock="$root/.terraform.lock.hcl"
  else
    lock=".terraform.lock.hcl"
  fi
  capture_lock "$lock" || return 1
  prepare_data "$data" || return 1
  write_config "$config" "$NATIVE_CACHE" || return 1
  first_output="$SCRATCH/first.jsonl"
  first_error="$SCRATCH/first.stderr"
  native_init "$tofu" "$root" "$data" "$home" "$config" "$NATIVE_CACHE" \
    "$first_output" "$first_error"
  status=$?
  relay "$first_output" "$first_error" || return 1
  lock_state_unchanged "$lock" "$LOCK_SNAPSHOT" "$LOCK_PRESENT" || return 1
  test "$status" -eq 0 && return 0
  test "$CACHE_USABLE" -eq 1 || return "$status"
  test "$status" -eq 1 || return "$status"
  test ! -s "$first_error" || return "$status"
  classifier "$first_output" "$NATIVE_CACHE"
  classify_status=$?
  test "$classify_status" -eq 0 || return "$status"
  quarantine_data "$data" || return 1
  retry_home="$SCRATCH/home"
  retry_config="$retry_home/cli.tfrc"
  ensure_dir "$retry_home" || return 1
  owned_dir "$retry_home" || return 1
  write_config "$retry_config" "" || return 1
  lock_state_unchanged "$lock" "$LOCK_SNAPSHOT" "$LOCK_PRESENT" || return 1
  retry_output="$SCRATCH/retry.jsonl"
  retry_error="$SCRATCH/retry.stderr"
  native_init "$tofu" "$root" "$data" "$retry_home" "$retry_config" \
    "" "$retry_output" "$retry_error"
  status=$?
  relay "$retry_output" "$retry_error" || return 1
  lock_state_unchanged "$lock" "$LOCK_SNAPSHOT" "$LOCK_PRESENT" || return 1
  test "$status" -eq 0 || return "$status"
  path_ok "$data" && test -d "$data" && test ! -L "$data"
}
main "$@"
