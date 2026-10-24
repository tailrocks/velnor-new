#!/usr/bin/env bash
# Guard the conventional tar entrypoint and preserve GNU tar as the backend.
set -euo pipefail

# The former wrapper ignored TAR_OPTIONS. Clear it before command dispatch so
# it cannot turn a plain invocation into an unpreflighted -P extraction.
unset TAR_OPTIONS || true

native_tar=/usr/bin/tar.gnu
guard_dir=/usr/local/libexec/velnor
die() {
  printf 'velnor-tar: %s\n' "$1" >&2
  exit 2
}

args=("$@")
invocation_dir="$(pwd -P)"
parse_dir="$invocation_dir"
legacy=0
if [ "${#args[@]}" -gt 0 ] && [[ "${args[0]}" =~ ^[A-Za-z]+$ ]]; then
  args[0]="-${args[0]}"
  legacy=1
fi

mode=""
mode_conflict=0
absolute=0
archive=""
archive_seen=0
archive_base=""
destination=""
destination_seen=0
destination_steps=()
files_from=""
files_from_seen=0
has_selectors=0
program=""
program_seen=0
compressor=""
unknown_option=""
preflight_unsupported=""
absolute_candidate=0
extract_candidate=0
ambiguous_dispatch=0
parse_error=""
selectors=()
native_args=()

record_parse_error() {
  [ -n "$parse_error" ] || parse_error="$1"
}

is_long_prefix() {
  local token="$1" option="$2"
  [ "$token" != -- ] && [ "${#token}" -gt 2 ] && [[ "$option" == "$token"* ]]
}

set_mode() {
  local next="$1"
  [ "$next" != x ] || extract_candidate=1
  if [ -n "$mode" ] && [ "$mode" != "$next" ]; then
    mode_conflict=1
  else
    mode="$next"
  fi
}

set_compressor() {
  local next="$1"
  if [ -n "$compressor" ] && [ "$compressor" != "$next" ]; then
    record_parse_error "multiple compression modes are unsupported"
    return
  fi
  compressor="$next"
}

consume_next() {
  if (( i + 1 >= ${#args[@]} )); then
    value=""
    record_parse_error "missing option value"
    return
  fi
  i=$((i + 1))
  value="${args[$i]}"
}

resolve_parse_path() {
  local path="$1"
  if [[ "$path" == /* ]]; then
    realpath -e -- "$path"
  else
    realpath -e -- "$parse_dir/$path"
  fi
}

resolve_invocation_path() {
  local path="$1"
  if [[ "$path" == /* ]]; then
    realpath -e -- "$path"
  else
    realpath -e -- "$invocation_dir/$path"
  fi
}

set_archive() {
  if [ -z "$1" ]; then
    record_parse_error "empty archive file"
    return
  fi
  if [ "$archive_seen" -ne 0 ]; then
    record_parse_error "multiple archive files are unsupported"
    return
  fi
  archive="$1"
  archive_base="$invocation_dir"
  archive_seen=1
}

set_destination() {
  if [ -z "$1" ]; then
    record_parse_error "empty extraction directory"
    return
  fi
  [ "$has_selectors" -eq 0 ] || record_parse_error "directory changes after selection are unsupported"
  destination_steps+=("$1")
  destination_seen=1
}

set_files_from() {
  if [ -z "$1" ]; then
    record_parse_error "empty files-from path"
    return
  fi
  if [ "$files_from_seen" -ne 0 ]; then
    record_parse_error "multiple files-from lists are unsupported"
    return
  fi
  files_from="$1"
  files_from_seen=1
  has_selectors=1
}

i=0
while [ "$i" -lt "${#args[@]}" ]; do
  arg="${args[$i]}"
  candidate_name="$arg"
  [[ "$candidate_name" != *=* ]] || candidate_name="${candidate_name%%=*}"
  if is_long_prefix "$candidate_name" --extract || is_long_prefix "$candidate_name" --get; then
    extract_candidate=1
  fi
  if is_long_prefix "$candidate_name" --absolute-names; then
    absolute_candidate=1
  fi
  case "$arg" in
    --)
      i=$((i + 1))
      while [ "$i" -lt "${#args[@]}" ]; do
        has_selectors=1
        selectors+=("${args[$i]}")
        i=$((i + 1))
      done
      break
      ;;
    --extract | --get)
      set_mode x
      native_args+=("$arg")
      ;;
    --create)
      set_mode c
      native_args+=("$arg")
      ;;
    --list)
      set_mode t
      native_args+=("$arg")
      ;;
    --append | --concatenate | --delete | --update)
      set_mode other
      native_args+=("$arg")
      ;;
    -T)
      consume_next
      set_files_from "$value"
      ;;
    -I)
      consume_next
      [ -n "$value" ] || record_parse_error "empty compression program"
      [ "$program_seen" -eq 0 ] || record_parse_error "multiple compression programs are unsupported"
      program="$value"
      program_seen=1
      native_args+=(--use-compress-program "$value")
      ;;
    -[!-]* )
      cluster="${arg#-}"
      legacy_cluster=0
      if [ "$i" -eq 0 ] && [ "$legacy" -eq 1 ]; then
        legacy_cluster=1
      fi
      k=0
      forward_cluster=""
      while [ "$k" -lt "${#cluster}" ]; do
        flag="${cluster:$k:1}"
        rest="${cluster:$((k + 1))}"
        case "$flag" in
          A | d | r | u) set_mode other; forward_cluster+="$flag" ;;
          a) preflight_unsupported="-a"; forward_cluster+=a ;;
          c) set_mode c; forward_cluster+=c ;;
          x) set_mode x; forward_cluster+=x ;;
          t) set_mode t; forward_cluster+=t ;;
          P) absolute=1; absolute_candidate=1; forward_cluster+=P ;;
          z) set_compressor gzip; forward_cluster+=z ;;
          j) set_compressor bzip2; forward_cluster+=j ;;
          J) set_compressor xz; forward_cluster+=J ;;
          Z) set_compressor compress; forward_cluster+=Z ;;
          v) forward_cluster+=v ;;
          f | C | T | I)
            if [ -n "$rest" ] && [ "$legacy_cluster" -eq 0 ]; then
              value="$rest"
              k="${#cluster}"
            else
              consume_next
            fi
            if [ "$flag" = f ]; then
              set_archive "$value"
            elif [ "$flag" = C ]; then
              set_destination "$value"
            elif [ "$flag" = T ]; then
              set_files_from "$value"
            else
              [ "$program_seen" -eq 0 ] || record_parse_error "multiple compression programs are unsupported"
              program="${value#=}"
              [ -n "$program" ] || record_parse_error "empty compression program"
              program_seen=1
              native_args+=(--use-compress-program "$program")
            fi
            ;;
          b | F | g | H | K | L | N | V | X)
            if [ -n "$rest" ] && [ "$legacy_cluster" -eq 0 ]; then
              value="$rest"
              k="${#cluster}"
            else
              consume_next
            fi
            preflight_unsupported="-$flag"
            native_args+=("-$flag" "$value")
            if [ "$legacy_cluster" -eq 0 ]; then
              k="${#cluster}"
            fi
            ;;
          B | d | G | h | i | k | l | M | m | o | O | p | q | R | s | S | U | w | W)
            preflight_unsupported="-$flag"
            forward_cluster+="$flag"
            ;;
          *)
            unknown_option="${unknown_option:--$flag}"
            if [ "$((i + 1))" -lt "${#args[@]}" ] && [ "${args[$((i + 1))]}" = -- ]; then
              ambiguous_dispatch=1
              i=$((i + 1))
            fi
            ;;
        esac
        k=$((k + 1))
      done
      [ -z "$forward_cluster" ] || native_args+=("-$forward_cluster")
      ;;
    --file=*)
      value="${arg#--file=}"
      [ -n "$value" ] || record_parse_error "empty archive file"
      set_archive "$value"
      ;;
    --file)
      consume_next
      set_archive "$value"
      ;;
    --directory=*)
      value="${arg#--directory=}"
      [ -n "$value" ] || record_parse_error "empty extraction directory"
      set_destination "$value"
      ;;
    --directory | --dir)
      consume_next
      set_destination "$value"
      ;;
    --dir=*)
      value="${arg#--dir=}"
      [ -n "$value" ] || record_parse_error "empty extraction directory"
      set_destination "$value"
      ;;
    --files-from=*)
      value="${arg#--files-from=}"
      [ -n "$value" ] || record_parse_error "empty files-from path"
      set_files_from "$value"
      ;;
    --files-from)
      consume_next
      set_files_from "$value"
      ;;
    --use-compress-program=*)
      [ "$program_seen" -eq 0 ] || record_parse_error "multiple compression programs are unsupported"
      program="${arg#--use-compress-program=}"
      [ -n "$program" ] || record_parse_error "empty compression program"
      program_seen=1
      native_args+=("$arg")
      ;;
    --use-compress-program)
      consume_next
      [ "$program_seen" -eq 0 ] || record_parse_error "multiple compression programs are unsupported"
      program="$value"
      program_seen=1
      native_args+=("$arg" "$value")
      ;;
    --gzip | --gunzip | --ungzip)
      set_compressor gzip
      native_args+=("$arg")
      ;;
    --bzip2)
      set_compressor bzip2
      native_args+=("$arg")
      ;;
    --xz)
      set_compressor xz
      native_args+=("$arg")
      ;;
    --lzma | --lzip | --lzop | --compress | --uncompress)
      case "$arg" in
        --lzma) set_compressor lzma ;;
        --lzip) set_compressor lzip ;;
        --lzop) set_compressor lzop ;;
        *) set_compressor compress ;;
      esac
      native_args+=("$arg")
      ;;
    --zstd)
      set_compressor zstd
      native_args+=("$arg")
      ;;
    --atime-preserve | --backup | --checkpoint | --totals)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      ;;
    --atime-preserve=* | --backup=* | --checkpoint=* | --totals=*)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      ;;
    -a | --auto-compress) native_args+=("$arg") ;;
    -P | --absolute-names) absolute=1; absolute_candidate=1; native_args+=("$arg") ;;
    --verbatim-files-from) native_args+=("$arg") ;;
    --no-verbatim-files-from | --null | --no-null)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      ;;
    --wildcards | --wildcards-match-slash | --no-anchored | --ignore-case \
      | --no-recursion | --recursive-unlink | --strip-components=* \
      | --strip-components | --transform=* | --transform | --xform=* | --xform \
      | --exclude=* | --exclude | --exclude-from=* | --exclude-from \
      | --occurrence=* | --occurrence | --ignore-zeros | --to-command=* \
      | --to-command | --posix | --format=* | --format)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      if [[ "$arg" = --exclude || "$arg" = --exclude-from || "$arg" = --strip-components || "$arg" = --transform || "$arg" = --xform || "$arg" = --to-command || "$arg" = --format ]]; then
        consume_next
        native_args+=("$value")
      fi
      ;;
    --anchored | --no-wildcards) native_args+=("$arg") ;;
    --exclude=*) native_args+=("$arg"); preflight_unsupported="$arg" ;;
    --exclude)
      consume_next
      native_args+=("$arg" "$value")
      preflight_unsupported="$arg"
      ;;
    --exclude-from=*) native_args+=("$arg"); preflight_unsupported="$arg" ;;
    --exclude-from)
      consume_next
      native_args+=("$arg" "$value")
      preflight_unsupported="$arg"
      ;;
    --add-file=* | --after-date=* | --blocking-factor=* | --checkpoint-action=* \
      | --group=* | --index-file=* | --label=* | --level=* | --listed-incremental=* \
      | --mode=* | --mtime=* | --newer=* | --newer-mtime=* | --owner=* \
      | --pax-option=* | --quote-chars=* | --quoting-style=* | --record-size=* \
      | --rmt-command=* | --rsh-command=* | --sparse-version=* | --starting-file=* \
      | --suffix=* | --warning=*)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      ;;
    --add-file | --after-date | --blocking-factor | --checkpoint-action \
      | --group | --index-file | --label | --level | --listed-incremental \
      | --mode | --mtime | --newer | --newer-mtime | --owner | --pax-option \
      | --quote-chars | --quoting-style | --record-size | --rmt-command \
      | --rsh-command | --sparse-version | --starting-file | --suffix | --warning)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      consume_next
      native_args+=("$value")
      ;;
    --occurrence=*)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      ;;
    --occurrence)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      ;;
    --acls | --no-acls | --delay-directory-restore | --dereference \
      | --hard-dereference | --ignore-command-error | --ignore-failed-read \
      | --keep-directory-symlink | --keep-newer-files | --keep-old-files \
      | --no-check-device | --no-delay-directory-restore | --no-ignore-case \
      | --no-overwrite-dir | --no-same-owner | --no-same-permissions | --no-seek \
      | --numeric-owner | --one-file-system | --overwrite | --overwrite-dir \
      | --preserve-permissions | --preserve | --same-owner | --same-permissions \
      | --show-defaults | --show-omitted-dirs | --touch | --unlink-first \
      | --verify | --verbose | --fast-read | --old-archive)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      ;;
    --version | --help)
      preflight_unsupported="$arg"
      native_args+=("$arg")
      ;;
    --*)
      unknown_option="${unknown_option:-$arg}"
      native_args+=("$arg")
      if [ "$((i + 1))" -lt "${#args[@]}" ] && [ "${args[$((i + 1))]}" = -- ]; then
        ambiguous_dispatch=1
        i=$((i + 1))
      fi
      ;;
    -*)
      unknown_option="${unknown_option:-$arg}"
      native_args+=("$arg")
      # A bare/unclassified dash token before -- may affect whether the next
      # token is a value or the end-of-options marker.
      if [ "$((i + 1))" -lt "${#args[@]}" ] && [ "${args[$((i + 1))]}" = -- ]; then
        ambiguous_dispatch=1
        i=$((i + 1))
      fi
      ;;
    *)
      has_selectors=1
      selectors+=("$arg")
      ;;
  esac
  i=$((i + 1))
done

# Ambiguous unknown-option arity cannot safely use the native fallback: a
# following -- may be a value rather than the end-of-options marker.
[ "$ambiguous_dispatch" -eq 0 ] || die "ambiguous option arity before end-of-options marker"

# Keep ordinary operations and non-absolute extraction byte-for-byte native.
# The single walk above has already handled option values and literal --.
if [ "$extract_candidate" -ne 1 ] || [ "$absolute_candidate" -ne 1 ]; then
  exec "$native_tar" "$@"
fi

[ "$mode_conflict" -eq 0 ] || die "conflicting archive operations"
if [ "$mode" != x ]; then
  [ "$extract_candidate" -eq 0 ] || die "unsupported extraction operation $unknown_option"
  exec "$native_tar" "$@"
fi
[ -z "$parse_error" ] || die "$parse_error"
[ -z "$unknown_option" ] || die "unsupported extraction option $unknown_option"
[ -z "$preflight_unsupported" ] || die "unsupported guarded extraction option $preflight_unsupported"
[ "$archive_seen" -eq 1 ] || die "guarded extraction requires a named archive"
[ "$archive" != - ] || die "guarded extraction from standard input is unsupported"
[ "$files_from_seen" -eq 0 ] || [ "${#selectors[@]}" -eq 0 ] || die "mixed positional and files-from selection is unsupported"
[ "$program_seen" -eq 0 ] || [ -z "$compressor" ] || die "multiple compression modes are unsupported"

parse_dir="$invocation_dir"
for destination_step in "${destination_steps[@]}"; do
  destination="$(resolve_parse_path "$destination_step")" || die "extraction directory cannot be resolved"
  [ -d "$destination" ] || die "extraction destination is not a directory"
  parse_dir="$destination"
done

if [ "$files_from_seen" -eq 1 ] && [ "$files_from" != - ]; then
  files_from="$(resolve_invocation_path "$files_from")" || die "files-from list cannot be resolved"
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-guard.XXXXXX")" || die "cannot create guard workspace"
work="$(cd -- "$work" && pwd -P)" || die "cannot resolve guard workspace"
trap 'rm -rf -- "$work"' EXIT
selector_nul="$work/selectors.nul"
selector_text="$work/selectors.txt"
: >"$selector_nul"
: >"$selector_text"
declare -A emitted_selector_keys=()
append_selector() {
  local candidate="$1" key="selector:$1"
  [[ -n "${emitted_selector_keys["$key"]+present}" ]] && return 0
  emitted_selector_keys["$key"]=1
  printf '%s\0' "$candidate" >>"$selector_nul"
  printf '%s\n' "$candidate" >>"$selector_text"
}
if [ "$files_from_seen" -eq 1 ]; then
  [ "$files_from" != - ] || die "guarded extraction from standard input selection is unsupported"
  while IFS= read -r selector || [ -n "$selector" ]; do
    [ -n "$selector" ] || continue
    [[ "$selector" != *$'\n'* && "$selector" != *$'\t'* ]] || die "unsupported files-from member name"
    append_selector "$selector"
  done <"$files_from"
elif [ "${#selectors[@]}" -gt 0 ]; then
  for selector in "${selectors[@]}"; do
    [ -n "$selector" ] || die "empty member selector"
    [[ "$selector" != *$'\n'* && "$selector" != *$'\t'* ]] || die "unsupported member selector"
    append_selector "$selector"
  done
fi

if [[ "$archive" == /* ]]; then
  archive_path="$archive"
else
  archive_path="$archive_base/$archive"
fi
archive_path="$(realpath -e -- "$archive_path")" || die "archive cannot be resolved"
[ -f "$archive_path" ] || die "guarded extraction requires a regular archive file"
if [ "$destination_seen" -eq 1 ]; then
  dest="$destination"
  [ -d "$dest" ] || die "extraction destination is not a directory"
  native_args+=(--directory "$dest")
else
  dest="$(pwd -P)"
fi
preflight=("$guard_dir/tar-guard-preflight.sh" --archive "$archive_path" --destination "$dest" --selectors "$selector_nul")
[ "$has_selectors" -eq 1 ] && preflight+=(--selected)
[ "$absolute" -eq 1 ] && preflight+=(--absolute)
[ -z "$compressor" ] || preflight+=(--compressor "$compressor")
if [ "$program_seen" -eq 1 ]; then
  preflight+=(--program "$program")
fi
"${preflight[@]}"

native_args+=(--file "$archive_path" --no-wildcards --anchored)
if [ "$has_selectors" -eq 1 ]; then
  native_args+=(--verbatim-files-from --files-from "$selector_text")
fi
if "$native_tar" "${native_args[@]}"; then
  exit 0
else
  backend_status=$?
  exit "$backend_status"
fi
