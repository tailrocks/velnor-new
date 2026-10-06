#!/usr/bin/env bash
set -uo pipefail
umask 077
PATH=/usr/bin:/bin
export PATH

check_path() {
  local path="$1"
  case "$path" in
    /*) ;;
    *) return 1 ;;
  esac
  case "$path" in
    *$'\n'*|*$'\r'*|*'//'|*/../*|*/./*|*/.|*/..) return 1 ;;
  esac
  while test "$path" != /; do
    test ! -L "$path" || return 1
    path="${path%/*}"
    test -n "$path" || path=/
  done
  return 0
}

relative_path_ok() {
  local path="$1" part
  local -a parts
  case "$path" in
    ''|/*|-*|*$'\n'*|*$'\r'*) return 1 ;;
  esac
  IFS=/ read -r -a parts <<< "$path"
  for part in "${parts[@]}"; do
    test -n "$part" && test "$part" != . && test "$part" != .. || return 1
  done
}

paths_disjoint() {
  local left="$1" right="$2"
  test "$left" != "$right" || return 1
  case "$left/" in "$right/"*) return 1 ;; esac
  case "$right/" in "$left/"*) return 1 ;; esac
}

report() {
  local value="$1" output="${GITHUB_OUTPUT:-}" available=false verified=false
  local error=SOURCE_VERIFICATION_FAILED first=""
  if test "$value" = true; then
    verified=true
    error=NONE
    if test -n "${VELNOR_TOFU_PROVIDER_OUTPUT:-}" \
      && check_path "$VELNOR_TOFU_PROVIDER_OUTPUT" \
      && test -d "$VELNOR_TOFU_PROVIDER_OUTPUT" \
      && first="$(find "$VELNOR_TOFU_PROVIDER_OUTPUT" -type f -print -quit 2>/dev/null)" \
      && test -n "$first"; then
      available=true
    else
      verified=false
      error=SOURCE_VERIFICATION_FAILED
    fi
  fi
  test -n "$output" || return 0
  check_path "$output" || return 0
  test -f "$output" || return 0
  printf "cache_available=%s\nverified=%s\nerror=%s\n" \
    "$available" "$verified" "$error" >> "$output" || true
}

decode_octal() {
  local value="$1" target="$2" temporary
  test -n "$value" || return 1
  test "$(( ${#value} % 4 ))" -eq 0 || return 1
  case "$value" in
    \\*) ;;
    *) return 1 ;;
  esac
  case "$value" in
    *[!0-7\\]*) return 1 ;;
  esac
  check_path "$target" || return 1
  temporary="$target.tmp"
  check_path "$temporary" || return 1
  printf '%b' "$value" > "$temporary" || return 1
  mv -f "$temporary" "$target" || return 1
}

safe_source() {
  local source="$1" authority remainder namespace provider
  authority="${source%%/*}"
  remainder="${source#*/}"
  namespace="${remainder%%/*}"
  provider="${remainder#*/}"
  case "$authority" in
    registry.opentofu.org|registry.terraform.io) ;;
    *) return 1 ;;
  esac
  test "$remainder" != "$provider" || return 1
  case "$namespace" in
    ''|*[!a-z0-9-]*) return 1 ;;
  esac
  case "$provider" in
    ''|*[!a-z0-9-]*) return 1 ;;
  esac
}

safe_version() {
  local version="$1" core suffix major minor patch extra
  case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) return 1 ;;
  esac
  case "$version" in
    *[!a-z0-9.+-]*) return 1 ;;
  esac
  core="${version%%[-+]*}"
  suffix="${version#"$core"}"
  test -z "$suffix" || test "${#suffix}" -gt 1 || return 1
  IFS=. read -r major minor patch extra <<< "$core"
  test -n "$major" && test -n "$minor" && test -n "$patch" && test -z "$extra" || return 1
  case "$major$minor$patch" in
    *[!0-9]*) return 1 ;;
  esac
}

copy_package() {
  local source="$1" target="$2" required="$3"
  local file relative destination bad file_list copied=0
  check_path "$source" || return 1
  test -d "$source" || return 1
  bad="$WORK/check.list"
  find "$source" \( ! -type f -a ! -type d \) -o \( -type f -links +1 \) > "$bad" || return 1
  test ! -s "$bad" || return 1
  file_list="$WORK/files.list"
  find "$source" -type f -print0 > "$file_list" || return 1
  if test ! -s "$file_list"; then
    test "$required" = no || return 1
    return 0
  fi
  while IFS= read -r -d '' file; do
    test -f "$file" || return 1
    check_path "$file" || return 1
    relative="${file#"$source"/}"
    test "$relative" != "$file" || return 1
    destination="$target/$relative"
    check_path "$destination" || return 1
    mkdir -p "${destination%/*}" || return 1
    check_path "${destination%/*}" || return 1
    test ! -e "$destination" && test ! -L "$destination" || return 1
    cat "$file" > "$destination" || return 1
    copied=$((copied + 1))
  done < "$file_list"
  test "$copied" -gt 0 || return 1
}

copy_version() {
  local input="$1" output="$2" source="$3" version="$4" platform="$5" required="$6"
  local package
  package="$input/$source/$version/$platform"
  check_path "$package" || return 1
  if test ! -e "$package" && test ! -L "$package"; then
    test "$required" = no || return 1
    return 0
  fi
  test -d "$package" || return 1
  copy_package "$package" "$output/$source/$version/$platform" "$required"
}

platform_for_target() {
  case "$1" in
    x86_64-unknown-linux-gnu) printf 'linux_amd64\n' ;;
    aarch64-unknown-linux-gnu) printf 'linux_arm64\n' ;;
    x86_64-apple-darwin) printf 'darwin_amd64\n' ;;
    aarch64-apple-darwin) printf 'darwin_arm64\n' ;;
    *) return 1 ;;
  esac
}

platform_for_host() {
  local os arch
  os="$(/usr/bin/uname -s)" || return 1
  arch="$(/usr/bin/uname -m)" || return 1
  case "$os:$arch" in
    Linux:x86_64|Linux:amd64) printf 'linux_amd64\n' ;;
    Linux:aarch64|Linux:arm64) printf 'linux_arm64\n' ;;
    Darwin:x86_64|Darwin:amd64) printf 'darwin_amd64\n' ;;
    Darwin:arm64|Darwin:aarch64) printf 'darwin_arm64\n' ;;
    *) return 1 ;;
  esac
}

copy_selected() {
  local input="$1" output="$2" required="$3" platform="$4" config_file="$5"
  local line source='' version=''
  test -d "$input" || return 1
  check_path "$input" || return 1
  check_path "$config_file" || return 1
  test -f "$config_file" || return 1
  mkdir -p "$output" || return 1
  check_path "$output" || return 1
  while IFS= read -r line || test -n "$line"; do
    if [[ "$line" =~ ^[[:space:]]*source[[:space:]]*=[[:space:]]\"([^\"]+)\"[[:space:]]*$ ]]; then
      source="${BASH_REMATCH[1]}"
      safe_source "$source" || return 1
    elif [[ "$line" =~ ^[[:space:]]*version[[:space:]]*=[[:space:]]\"=[[:space:]]([^\"]+)\"[[:space:]]*$ ]]; then
      version="${BASH_REMATCH[1]}"
      safe_version "$version" || return 1
      test -n "$source" || return 1
      copy_version "$input" "$output" "$source" "$version" "$platform" "$required" || return 1
      source=''
      version=''
    fi
  done < "$config_file"
  test -z "$source" && test -z "$version"
}

normalize_tree() {
  local root="$1" bad
  bad="$WORK/normalize.list"
  find "$root" \( ! -type f -a ! -type d \) -o \( -type f -links +1 \) > "$bad" || return 1
  test ! -s "$bad" || return 1
  find "$root" -type f -exec chmod 0755 {} + || return 1
  find "$root" -type d -exec chmod 0755 {} + || return 1
  TZ=UTC find "$root" -exec touch -t 197001010000.00 {} + || return 1
}

native_init() {
  local tofu_bin="$1" root="$2" temp="$3" candidate_cache="$4"
  local captured="$5" mise_home="$6" tofu_data="$7" cli
  cli="$temp/cli.tfrc"
  cat > "$cli" <<EOF
plugin_cache_dir = "$candidate_cache"
disable_checkpoint = true
provider_installation {
  direct {}
}
EOF
  check_path "$cli" || return 1
  env -i PATH="$PATH" HOME="$mise_home" TF_DATA_DIR="$tofu_data" \
    TF_CLI_CONFIG_FILE="$cli" TF_PLUGIN_CACHE_DIR="$candidate_cache" \
    TF_IN_AUTOMATION=1 TF_INPUT=0 NO_COLOR=1 \
    "$tofu_bin" "-chdir=$root" init -backend=false -input=false \
    -lockfile=readonly -no-color > "$temp/init.log" 2>&1 || return 1
  cmp -s "$captured" "$root/.terraform.lock.hcl"
}

producer() {
  local target="${1:-}" lock_arg="${2:-}" config_arg="${3:-}" tofu_version="${4:-}"
  local root_key="${5:-}" binary_rel="${6:-}"
  local candidate="${VELNOR_TOFU_PROVIDER_CANDIDATE:-}"
  local output="${VELNOR_TOFU_PROVIDER_OUTPUT:-}"
  local mise_data="${MISE_DATA_DIR:-}"
  local runner_temp="${RUNNER_TEMP:-}"
  local temp root candidate_cache final config lock captured platform host_platform
  local mise_home tofu_bin
  test "$#" -eq 6 && test -n "$output" && test -n "$mise_data" && test -n "$runner_temp" || return 1
  case "$root_key" in dir-*) ;; *) return 1 ;; esac
  case "${root_key#dir-}" in *[!a-f0-9]*) return 1 ;; esac
  safe_version "$tofu_version" || return 1
  relative_path_ok "$binary_rel" || return 1
  test "${VELNOR_TOFU_PROVIDER_TARGET:-}" = "$target" || return 1
  platform="$(platform_for_target "$target")" || return 1
  host_platform="$(platform_for_host)" || return 1
  test "$host_platform" = "$platform" || return 1
  check_path "$mise_data" || return 1
  check_path "$runner_temp" || return 1
  check_path "$output" || return 1
  test -z "$candidate" || check_path "$candidate" || return 1
  test "$candidate" != "$output" || return 1
  test -z "$candidate" || paths_disjoint "$candidate" "$output" || return 1
  paths_disjoint "$mise_data" "$output" || return 1
  test ! -e "$output" || return 1
  tofu_bin="$mise_data/$binary_rel"
  check_path "$tofu_bin" || return 1
  test -f "$tofu_bin" && test -x "$tofu_bin" && test ! -L "$tofu_bin" || return 1
  temp="$(mktemp -d "$runner_temp/velnor-tofu-provider.XXXXXX")" || return 1
  check_path "$temp" || return 1
  WORK="$temp"
  trap 'test -n "${WORK:-}" && rm -rf -- "$WORK"' EXIT HUP INT TERM
  root="$temp/root"
  candidate_cache="$temp/candidate"
  final="$temp/final"
  mkdir -p "$root" "$candidate_cache" "$final" "$temp/home" "$temp/data" \
    "$temp/mise-home" || return 1
  check_path "$root" && check_path "$candidate_cache" && check_path "$final" || return 1
  config="$root/main.tf"
  lock="$root/.terraform.lock.hcl"
  captured="$temp/captured.lock"
  mise_home="$temp/mise-home"
  decode_octal "$config_arg" "$config" || return 1
  decode_octal "$lock_arg" "$captured" || return 1
  cp -P "$captured" "$lock" || return 1
  printf '%s' "$root_key" > "$temp/expected-root-key" || return 1
  if test -n "$candidate"; then
    if test -e "$candidate" || test -L "$candidate"; then
      test -d "$candidate" || return 1
      test -f "$candidate/.velnor-root-key" && test ! -L "$candidate/.velnor-root-key" || return 1
      test "$(find "$candidate/.velnor-root-key" -type f -links 1 -print)" = "$candidate/.velnor-root-key" || return 1
      cmp -s "$temp/expected-root-key" "$candidate/.velnor-root-key" || return 1
      copy_selected "$candidate" "$candidate_cache" no "$platform" "$config" || return 1
    fi
  fi
  native_init "$tofu_bin" "$root" "$temp" "$candidate_cache" "$captured" \
    "$mise_home" "$temp/data" || return 1
  copy_selected "$candidate_cache" "$final" yes "$platform" "$config" || return 1
  cat "$temp/expected-root-key" > "$final/.velnor-root-key" || return 1
  chmod 0600 "$final/.velnor-root-key" || return 1
  test ! -L "$final/.velnor-root-key" || return 1
  normalize_tree "$final" || return 1
  mkdir -p "${output%/*}" || return 1
  check_path "${output%/*}" || return 1
  mv "$final" "$output" || return 1
}
status=0
if producer "${1:-}" "${2:-}" "${3:-}" "${4:-}" "${5:-}" "${6:-}"; then
  report true
else
  status=$?
  report false
fi
exit "$status"
