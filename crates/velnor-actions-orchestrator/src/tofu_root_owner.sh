set -eu
umask 077
root_key="$1"
check_path() {
  path="$1"
  case "$path" in /*) ;; *) echo 'tofu_isolation_relative_path' >&2; exit 1;; esac
  while test -n "$path" && test "$path" != /; do
    test ! -L "$path" || { echo 'tofu_isolation_symlink' >&2; exit 1; }
    path="${path%/*}"
  done
}
own_directory() {
  directory="$1"
  check_path "$directory"
  if test -e "$directory"; then test -d "$directory" || exit 1; else mkdir -p "$directory"; fi
  check_path "$directory"
  marker="$directory/.velnor-root-key"
  if test -e "$marker" || test -L "$marker"; then
    test -f "$marker" && test ! -L "$marker" || exit 1
    test "$(find "$marker" -type f -links 1 -print)" = "$marker" || exit 1
    printf '%s' "$root_key" | cmp -s - "$marker" || { echo 'tofu_root_owner_mismatch' >&2; exit 1; }
  else
    test -z "$(find "$directory" -mindepth 1 -print -quit)" || { echo 'tofu_root_owner_missing' >&2; exit 1; }
    (set -C; printf '%s' "$root_key" > "$marker") || exit 1
  fi
}
