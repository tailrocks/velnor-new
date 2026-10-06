#!/bin/bash
# GNU tar 1.35 stats and extracts with openat2. qemu-user returns ENOSYS and
# glibc does not fall back. BusyBox tar works. Flags that change archive
# bytes and are not implemented fail closed. -v is non-semantic: it does
# not change archive bytes.
set -euo pipefail

mode=""
archive=""
chdir=""
files_from=""
gzip=0
zstd=0
program=""
absolute=0
posix=0
legacy_first_arg=0
excludes=()
positionals=()
args=("$@")
i=0

# GNU tar accepts a leading dashless cluster: `tar xz -C dir -f archive`.
if [ "${#args[@]}" -gt 0 ]; then
  first="${args[0]}"
  if [[ "$first" != -* && "$first" =~ ^[A-Za-z]+$ ]]; then
    legacy_first_arg=1
    args=("-${first}" "${args[@]:1}")
  fi
fi

die() {
  printf 'velnor-tar: %s\n' "$1" >&2
  exit 1
}

need() {
  i=$((i + 1))
  if [ "$i" -ge "${#args[@]}" ]; then
    die "missing value for ${args[$((i - 1))]}"
  fi
}

while [ "$i" -lt "${#args[@]}" ]; do
  arg="${args[$i]}"
  arg_legacy=0
  if [ "$i" -eq 0 ] && [ "$legacy_first_arg" -eq 1 ]; then
    arg_legacy=1
    legacy_first_arg=0
  fi
  case "$arg" in
    --absolute-names | -P)
      absolute=1
      ;;
    --zstd)
      zstd=1
      ;;
    --posix)
      # Create already goes through the pax writer. Extract stays fail-closed.
      posix=1
      ;;
    --delay-directory-restore | --force-local | --no-same-owner | --no-same-permissions | --numeric-owner | --overwrite)
      die "unsupported option $arg"
      ;;
    --version)
      printf 'velnor-tar busybox\n'
      exit 0
      ;;
    --warning | --warning=*)
      die "unsupported option $arg"
      ;;
    --exclude)
      need
      excludes+=("${args[$i]}")
      ;;
    --exclude=*)
      excludes+=("${arg#--exclude=}")
      ;;
    --files-from)
      need
      files_from="${args[$i]}"
      ;;
    --files-from=*)
      files_from="${arg#--files-from=}"
      ;;
    --use-compress-program)
      need
      program="${args[$i]}"
      ;;
    --use-compress-program=*)
      program="${arg#--use-compress-program=}"
      ;;
    -C)
      need
      chdir="${args[$i]}"
      ;;
    -f)
      need
      archive="${args[$i]}"
      ;;
    -z)
      gzip=1
      ;;
    -c | --create)
      mode=c
      ;;
    -x | --extract | --get)
      mode=x
      ;;
    -t | --list)
      mode=t
      ;;
    --)
      i=$((i + 1))
      while [ "$i" -lt "${#args[@]}" ]; do
        positionals+=("${args[$i]}")
        i=$((i + 1))
      done
      break
      ;;
    -[^-]*)
      cluster="${arg#-}"
      cluster_legacy="$arg_legacy"
      k=0
      while [ "$k" -lt "${#cluster}" ]; do
        flag="${cluster:$k:1}"
        case "$flag" in
          c) mode=c ;;
          x) mode=x ;;
          t) mode=t ;;
          z) gzip=1 ;;
          P) absolute=1 ;;
          # -v is non-semantic. Verbose text is not part of the archive.
          v) ;;
          j | J | Z | h | m | o | k | O | a)
            die "unsupported flag -$flag"
            ;;
          f | C)
            # In dashless old-style clusters, later flag letters stay flags.
            # In dashed GNU clusters, -f/-C consume the complete remainder.
            rest="${cluster:$((k + 1))}"
            attached=0
            if [ -n "$rest" ] && [ "$cluster_legacy" -eq 0 ]; then
              attached=1
            fi
            if [ "$attached" -eq 1 ]; then
              value="$rest"
            else
              need
              value="${args[$i]}"
            fi
            if [ "$flag" = f ]; then
              archive="$value"
            else
              chdir="$value"
            fi
            if [ "$attached" -eq 1 ]; then
              break
            fi
            ;;
          *)
            die "unsupported flag -$flag"
            ;;
        esac
        k=$((k + 1))
      done
      ;;
    -*)
      die "unsupported option $arg"
      ;;
    *)
      positionals+=("$arg")
      ;;
  esac
  i=$((i + 1))
done

if [ -z "$mode" ]; then
  die "missing c, x, or t"
fi

files=()
if [ -n "$files_from" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    if [ -n "$line" ]; then
      files+=("$line")
    fi
  done <"$files_from"
elif [ "${#positionals[@]}" -gt 0 ]; then
  files=("${positionals[@]}")
fi

filtered=()
if [ "${#files[@]}" -gt 0 ]; then
  for path in "${files[@]}"; do
    skip=0
    if [ "${#excludes[@]}" -gt 0 ]; then
      for excluded in "${excludes[@]}"; do
        if [ "$path" = "$excluded" ] || [ "$path" = "${excluded##*/}" ]; then
          skip=1
        fi
      done
    fi
    if [ "$skip" -eq 0 ]; then
      filtered+=("$path")
    fi
  done
fi

bb=(busybox tar "$mode")
if [ "$gzip" -eq 1 ]; then
  bb+=(-z)
fi
if [ "$zstd" -eq 1 ] && [ -z "$program" ]; then
  if [ "$mode" = c ]; then
    program="zstd -c"
  else
    program="zstd -d -c"
  fi
fi

if [ -n "$program" ]; then
  bb+=(-f -)
elif [ -n "$archive" ]; then
  bb+=(-f "$archive")
fi
if [ -n "$chdir" ]; then
  bb+=(-C "$chdir")
fi
# A --files-from list can exceed ARG_MAX. Do not put it on the BusyBox argv.
# tar-absolute.sh reads that list from the file instead.
if [ -z "$files_from" ] && [ "${#filtered[@]}" -gt 0 ]; then
  bb+=("${filtered[@]}")
fi

_velnor_tar_here="$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"
# shellcheck disable=SC1091
. "$_velnor_tar_here/tar-absolute.sh"
finish_tar
