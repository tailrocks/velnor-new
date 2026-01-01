#!/bin/bash
# GNU tar 1.35 stats and extracts with openat2. qemu-user returns ENOSYS and
# glibc does not fall back. BusyBox tar works. actions/cache and dpkg pass
# GNU-only flags, so accept those and run BusyBox.
set -euo pipefail

mode=""
archive=""
chdir=""
files_from=""
gzip=0
zstd=0
program=""
strip=""
excludes=()
positionals=()
args=("$@")
i=0

# GNU tar accepts a leading dashless cluster: `tar xz -C dir -f archive`.
if [ "${#args[@]}" -gt 0 ]; then
  first="${args[0]}"
  if [[ "$first" != -* && "$first" =~ ^[A-Za-z]+$ ]]; then
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
  case "$arg" in
    --posix | -P | --delay-directory-restore | --force-local | --no-same-owner | --no-same-permissions | --numeric-owner | --overwrite | --zstd)
      if [ "$arg" = "--zstd" ]; then
        zstd=1
      fi
      ;;
    --version)
      printf 'velnor-tar busybox\n'
      exit 0
      ;;
    --warning | --warning=*)
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
    --strip-components)
      need
      strip="${args[$i]}"
      ;;
    --strip-components=*)
      strip="${arg#--strip-components=}"
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
      k=0
      while [ "$k" -lt "${#cluster}" ]; do
        flag="${cluster:$k:1}"
        case "$flag" in
          c) mode=c ;;
          x) mode=x ;;
          t) mode=t ;;
          z) gzip=1 ;;
          j | J | Z | v | h | m | o | k | O | a | P) ;;
          f | C)
            rest="${cluster:$((k + 1))}"
            if [ -n "$rest" ]; then
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
            break
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
if [ -n "$strip" ]; then
  bb+=(--strip-components "$strip")
fi
if [ "${#filtered[@]}" -gt 0 ]; then
  bb+=("${filtered[@]}")
fi

if [ -n "$program" ]; then
  if [ "$mode" = c ]; then
    "${bb[@]}" | bash -c "$program" >"$archive"
  else
    bash -c "$program" <"$archive" | "${bb[@]}"
  fi
else
  exec "${bb[@]}"
fi
