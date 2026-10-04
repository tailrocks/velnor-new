resource_json_single_object() {
  local root="$1" file="$2" canonical identity links owner group bytes
  case "$file" in "$root"/*) ;; *) return 1 ;; esac
  [[ -d "$root" && ! -L "$root" && -f "$file" && ! -L "$file" ]] || return 1
  [[ "$(stat -c '%a:%u:%g' -- "$root" 2>/dev/null)" == "700:$(id -u):$(id -g)" ]] || return 1
  canonical="$(realpath -e -- "$file" 2>/dev/null)" || return 1
  [[ "$canonical" == "$file" ]] || return 1
  identity="$(stat -c '%h:%u:%g:%s' -- "$file" 2>/dev/null)" || return 1
  IFS=: read -r links owner group bytes <<< "$identity"
  [[ "$links" == 1 && "$owner:$group" == "$(id -u):$(id -g)" && "$bytes" =~ ^[0-9]{1,5}$ ]] || return 1
  (( 10#$bytes > 0 && 10#$bytes <= 65536 )) || return 1
  jq -se 'length == 1 and (.[0] | type == "object")' "$file" >/dev/null 2>&1
}

resource_capture_bounded() {
  local root="$1" file="$2"
  shift 2
  case "$file" in "$root"/*) ;; *) return 1 ;; esac
  [[ -d "$root" && ! -L "$root" && ! -e "$file" && ! -L "$file" ]] || return 1
  [[ "$(stat -c '%a:%u:%g' -- "$root" 2>/dev/null)" == "700:$(id -u):$(id -g)" ]] || return 1
  (umask 077; set -C; ulimit -f 128 && "$@" > "$file")
}
