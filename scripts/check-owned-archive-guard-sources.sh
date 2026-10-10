#!/usr/bin/env bash
# Sourced by the checkout-owned archive guard builder after its own path and
# checkout ancestry have been admitted. This uses only the shared input manifest.

check_source_file() {
  local relative="$1" optional="$2" full identity owner mode links size
  if ! check_source_parent_dirs "$relative" "$optional"; then
    return 1
  fi
  # The builder sets this trusted checkout root before sourcing this file.
  # shellcheck disable=SC2154
  full="$repository/$relative"
  if [[ -L "$full" ]]; then
    fail "archive guard source is a symlink: $relative"
  fi
  if [[ ! -e "$full" ]]; then
    [[ "$optional" == true ]] && return 1
    fail "required archive guard source is missing: $relative"
  fi
  [[ -f "$full" ]] || fail "archive guard source is not a regular file: $relative"
  identity="$(stat_source_identity "$full")" \
    || fail "cannot inspect archive guard source: $relative"
  read -r owner mode links size <<<"$identity"
  [[ "$owner" == "$(id -u)" && "$mode" =~ ^[0-7]+$ && "$links" == 1 \
    && "$size" =~ ^[0-9]+$ ]] \
    || fail "archive guard source has untrusted provenance: $relative"
  (( (8#$mode & 022) == 0 )) \
    || fail "archive guard source is writable by others: $relative"
  (( 10#$size <= 16777216 )) \
    || fail "archive guard source exceeds the file size limit: $relative"
  SOURCE_FILE_SIZE=$((10#$size))
}

check_source_parent_dirs() {
  local relative="$1" optional="$2" parent current component
  [[ "$relative" == */* ]] || return 0
  parent="${relative%/*}"
  current="$repository"
  while [[ -n "$parent" ]]; do
    if [[ "$parent" == */* ]]; then
      component="${parent%%/*}"
      parent="${parent#*/}"
    else
      component="$parent"
      parent=''
    fi
    current="$current/$component"
    if [[ ! -e "$current" && ! -L "$current" ]]; then
      [[ "$optional" == true ]] && return 1
      fail "required archive guard source parent is missing: $current"
    fi
    check_directory "$current"
  done
}

check_source_size() {
  source_file_count=$((source_file_count + 1))
  (( source_file_count <= 512 )) \
    || fail 'archive guard source file count exceeded'
  source_total_bytes=$((source_total_bytes + SOURCE_FILE_SIZE))
  (( source_total_bytes <= 33554432 )) \
    || fail 'archive guard source tree exceeds the total size limit'
}

stat_source_identity() {
  if [[ "$(uname -s)" == Darwin ]]; then
    stat -f '%u %Lp %l %z' "$1"
  else
    stat -c '%u %a %h %s' "$1"
  fi
}

validate_source_tree() {
  local relative="$1" full entry find_status discovered=0 directories=1
  full="$repository/$relative"
  check_source_parent_dirs "$relative" false
  check_directory "$full"
  temporary="$(mktemp "${TMPDIR:-/tmp}/archive-guard-find.XXXXXX")"
  if find "$full" -mindepth 1 -print0 >"$temporary"; then
    find_status=0
  else
    find_status=$?
  fi
  while IFS= read -r -d '' entry; do
    discovered=$((discovered + 1))
    (( discovered <= 512 )) \
      || fail "archive guard source tree entry count exceeded: $relative"
    if [[ -L "$entry" ]]; then
      fail "archive guard source tree contains a symlink: $entry"
    elif [[ -d "$entry" ]]; then
      check_directory "$entry"
      directories=$((directories + 1))
      (( directories <= 512 )) \
        || fail "archive guard source tree directory count exceeded: $relative"
    elif [[ -f "$entry" ]]; then
      check_source_file "${entry#"$repository"/}" false
      check_source_size
    else
      fail "archive guard source tree contains a special file: $entry"
    fi
  done <"$temporary"
  rm -f -- "$temporary"
  temporary=''
  if ((find_status != 0)); then
    fail "cannot enumerate archive guard source tree: $relative"
  fi
}

validate_source_manifest() {
  local manifest='scripts/archive-guard-inputs.txt' kind relative extra entries=0
  check_source_file "$manifest" false
  (( SOURCE_FILE_SIZE <= 32768 )) \
    || fail 'archive guard source manifest exceeds the input limit'
  source_total_bytes=$SOURCE_FILE_SIZE
  source_file_count=0
  while IFS=' ' read -r kind relative extra; do
    [[ -n "$kind" && -n "$relative" && -z "$extra" \
      && "$relative" =~ ^[A-Za-z0-9._/-]+$ ]] \
      || fail 'archive guard source manifest entry is malformed'
    case "/$relative/" in
      *"//"*|*"/../"*|*"/./"*) fail 'archive guard source path is unsafe' ;;
    esac
    entries=$((entries + 1))
    (( entries <= 512 )) || fail 'archive guard source manifest is too large'
    case "$kind" in
      file)
        check_source_file "$relative" false
        check_source_size
        ;;
      optional)
        if check_source_file "$relative" true; then
          check_source_size
        fi
        ;;
      tree) validate_source_tree "$relative" ;;
      *) fail 'archive guard source manifest kind is unknown' ;;
    esac
  done < "$repository/$manifest"
  (( entries > 0 )) || fail 'archive guard source manifest is empty'
}
