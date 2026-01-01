#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repository="$(cd -- "$script_dir/.." && pwd -P)"
script_path="$script_dir/$(basename -- "${BASH_SOURCE[0]}")"
if [[ -z "${HOME:-}" || "$HOME" != /* ]]; then
  printf 'archive guard build: an absolute HOME is required to provision Rust 1.98.1\n' >&2
  exit 1
fi
home_root="$(cd -- "$HOME" && pwd -P)"
state_root="$repository/.velnor"
guard_state="$state_root/archive-guard"
guard_bin_dir="$guard_state/bin"
cache_root="$home_root/.cache"
build_root="$cache_root/velnor/archive-guard"
guard_target_root="$build_root/target"
guard_target=''
build_tmp_root="$build_root/tmp"
run_tmp=''
temporary=''
cargo_home="$build_root/cargo-home"
mise_data="$home_root/.local/share/mise"
mise_cache="$build_root/mise-cache"
rustup_home="$build_root/rustup-home"

fail() {
  printf 'archive guard build: %s\n' "$1" >&2
  exit 1
}

cleanup() {
  if [[ -n "$temporary" ]]; then
    rm -f -- "$temporary"
  fi
  if [[ -n "$run_tmp" ]]; then
    rm -rf -- "$run_tmp"
  fi
}

trap cleanup EXIT

[[ "$script_path" == "$repository/scripts/build-owned-archive-guard.sh" ]] \
  || fail 'builder script is not the checkout-owned entrypoint'
if ! mise_bin="$(command -v mise)" || [[ "$mise_bin" != /* ]]; then
  fail 'mise is required to provision Rust 1.98.1'
fi
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
stat_identity() {
  if [[ "$(uname -s)" == Darwin ]]; then
    stat -f '%u %Lp' "$1"
  else
    stat -c '%u %a' "$1"
  fi
}

check_mise_executable() {
  local identity owner mode links directory physical owner_id
  [[ ! -L "$mise_bin" && -f "$mise_bin" && -x "$mise_bin" ]] \
    || fail 'Mise executable is missing or unsafe'
  if [[ "$(uname -s)" == Darwin ]]; then
    identity="$(stat -f '%u %Lp %l' "$mise_bin")" \
      || fail 'cannot inspect Mise executable'
  else
    identity="$(stat -c '%u %a %h' "$mise_bin")" \
      || fail 'cannot inspect Mise executable'
  fi
  read -r owner mode links <<<"$identity"
  owner_id="$(id -u)"
  [[ ("$owner" == "$owner_id" || "$owner" == 0) && "$mode" =~ ^[0-7]+$ \
    && "$links" == 1 ]] || fail 'Mise executable has untrusted provenance'
  (( (8#$mode & 022) == 0 )) || fail 'Mise executable is writable by others'
  directory="$(dirname -- "$mise_bin")"
  physical="$(cd -- "$directory" && pwd -P)" \
    || fail 'cannot inspect Mise executable directory'
  [[ "$physical/$(basename -- "$mise_bin")" == "$mise_bin" ]] \
    || fail 'Mise executable path contains a symlink'
  identity="$(stat_identity "$directory")" || fail 'cannot inspect Mise directory'
  read -r owner mode <<<"$identity"
  [[ ("$owner" == "$owner_id" || "$owner" == 0) && "$mode" =~ ^[0-7]+$ ]] \
    || fail 'Mise directory has untrusted ownership'
  (( (8#$mode & 022) == 0 )) || fail 'Mise directory is writable by others'
}

check_mise_executable

check_directory() {
  local path="$1" identity owner mode
  [[ ! -L "$path" && -d "$path" ]] || fail "unsafe build directory: $path"
  identity="$(stat_identity "$path")" || fail "cannot inspect build directory: $path"
  read -r owner mode <<<"$identity"
  [[ "$owner" == "$(id -u)" && "$mode" =~ ^[0-7]+$ ]] \
    || fail "build directory has untrusted ownership: $path"
  (( (8#$mode & 022) == 0 )) || fail "build directory is writable by others: $path"
}

check_safe_ancestor() {
  local path="$1" identity owner mode owner_id
  [[ ! -L "$path" && -d "$path" ]] \
    || fail "archive checkout ancestry is unsafe: $path"
  if [[ "$(uname -s)" == Darwin ]]; then
    identity="$(stat -f '%u %p' "$path")" \
      || fail "cannot inspect archive checkout ancestry: $path"
  else
    identity="$(stat_identity "$path")" \
      || fail "cannot inspect archive checkout ancestry: $path"
  fi
  read -r owner mode <<<"$identity"
  owner_id="$(id -u)"
  mode="$(printf '%o' "$((8#$mode & 07777))")"
  [[ ("$owner" == "$owner_id" || "$owner" == 0) && "$mode" =~ ^[0-7]+$ ]] \
    || fail "archive checkout ancestry has untrusted ownership: $path"
  if (( (8#$mode & 022) != 0 )); then
    if [[ "$owner" != 0 ]] || (( (8#$mode & 01000) == 0 )); then
      fail "archive checkout ancestry is writable by others: $path"
    fi
  fi
}

check_checkout_ancestry() {
  local ancestor
  check_directory "$repository"
  ancestor="$(dirname -- "$repository")"
  while :; do
    check_safe_ancestor "$ancestor"
    [[ "$ancestor" == / ]] && break
    ancestor="$(dirname -- "$ancestor")"
  done
}

check_source_checker() {
  local path="$repository/scripts/check-owned-archive-guard-sources.sh"
  local identity owner mode links size
  check_directory "$repository/scripts"
  [[ ! -L "$path" && -f "$path" ]] \
    || fail 'archive guard source checker is missing or unsafe'
  if [[ "$(uname -s)" == Darwin ]]; then
    identity="$(stat -f '%u %Lp %l %z' "$path")" \
      || fail 'cannot inspect archive guard source checker'
  else
    identity="$(stat -c '%u %a %h %s' "$path")" \
      || fail 'cannot inspect archive guard source checker'
  fi
  read -r owner mode links size <<<"$identity"
  [[ "$owner" == "$(id -u)" && "$mode" =~ ^[0-7]+$ && "$links" == 1 \
    && "$size" =~ ^[0-9]+$ ]] \
    || fail 'archive guard source checker has untrusted provenance'
  (( (8#$mode & 022) == 0 && 10#$size <= 16777216 )) \
    || fail 'archive guard source checker permissions or size are unsafe'
}

check_checkout_ancestry
check_source_checker
# shellcheck source=scripts/check-owned-archive-guard-sources.sh
source "$repository/scripts/check-owned-archive-guard-sources.sh"
validate_source_manifest
# shellcheck source=scripts/archive-guard-build-namespace.sh
source "$repository/scripts/archive-guard-build-namespace.sh"

check_config_file() {
  local path="$1" identity owner mode
  if [[ -L "$path" ]]; then
    fail "configuration path is a symlink: $path"
  fi
  if [[ -e "$path" ]]; then
    [[ -f "$path" ]] || fail "configuration is not a regular file: $path"
    identity="$(stat_identity "$path")" || fail "cannot inspect configuration: $path"
    read -r owner mode <<<"$identity"
    [[ "$owner" == "$(id -u)" && "$mode" =~ ^[0-7]+$ ]] \
      || fail "configuration has untrusted ownership: $path"
    (( (8#$mode & 022) == 0 )) || fail "configuration is writable by others: $path"
  fi
}

check_executable() {
  local path="$1" identity owner mode links
  [[ ! -L "$path" && -f "$path" && -x "$path" ]] \
    || fail "guard executable is missing or unsafe: $path"
  if [[ "$(uname -s)" == Darwin ]]; then
    identity="$(stat -f '%u %Lp %l' "$path")" \
      || fail "cannot inspect guard executable: $path"
  else
    identity="$(stat -c '%u %a %h' "$path")" \
      || fail "cannot inspect guard executable: $path"
  fi
  read -r owner mode links <<<"$identity"
  [[ "$owner" == "$(id -u)" && "$mode" =~ ^[0-7]+$ && "$links" == 1 ]] \
    || fail "guard executable has untrusted provenance: $path"
  (( (8#$mode & 022) == 0 && (8#$mode & 0111) != 0 )) \
    || fail "guard executable permissions are unsafe: $path"
}

check_cargo_output() {
  local path="$1" identity owner mode links
  [[ ! -L "$path" && -f "$path" && -x "$path" ]] \
    || fail "Cargo did not produce the guard executable: $path"
  if [[ "$(uname -s)" == Darwin ]]; then
    identity="$(stat -f '%u %Lp %l' "$path")" \
      || fail "cannot inspect Cargo output: $path"
  else
    identity="$(stat -c '%u %a %h' "$path")" \
      || fail "cannot inspect Cargo output: $path"
  fi
  read -r owner mode links <<<"$identity"
  [[ "$owner" == "$(id -u)" && "$mode" =~ ^[0-7]+$ && "$links" =~ ^[0-9]+$ ]] \
    || fail "Cargo output has untrusted provenance: $path"
  (( (8#$mode & 022) == 0 && (8#$mode & 0111) != 0 )) \
    || fail "Cargo output permissions are unsafe: $path"
}

check_external_build_inputs() {
  local ancestor="$1" config input
  while :; do
    for config in "$ancestor/.cargo/config" "$ancestor/.cargo/config.toml"; do
      if [[ -L "$ancestor/.cargo" || -L "$config" || -e "$config" ]]; then
        fail "Cargo configuration outside the checkout is forbidden: $config"
      fi
    done
    for input in "$ancestor/rust-toolchain" "$ancestor/rust-toolchain.toml"; do
      if [[ -L "$input" || -e "$input" ]]; then
        fail "Rust toolchain configuration outside the checkout is forbidden: $input"
      fi
    done
    [[ "$ancestor" == / ]] && break
    ancestor="$(dirname -- "$ancestor")"
  done
}

check_checkout_ancestry
validate_source_manifest
check_external_build_inputs "$(dirname -- "$repository")"
if [[ -L "$repository/.cargo" ]]; then
  fail 'checkout .cargo directory is a symlink'
fi
for input in \
  "$repository/.cargo/config" \
  "$repository/.cargo/config.toml" \
  "$repository/rust-toolchain" \
  "$repository/rust-toolchain.toml"; do
  check_config_file "$input"
done

if [[ -L "$state_root" ]]; then
  fail 'checkout .velnor directory is a symlink'
fi
for path in "$guard_state" "$guard_bin_dir" "$cache_root" "$cache_root/velnor" \
  "$build_root" "$guard_target_root" "$build_tmp_root" "$cargo_home" "$mise_data" \
  "$mise_cache" "$rustup_home"; do
  [[ ! -L "$path" ]] || fail "build path is a symlink: $path"
done
for path in "$home_root/.local" "$home_root/.local/share"; do
  [[ ! -L "$path" ]] || fail "Mise path is a symlink: $path"
done
mkdir -p -- "$guard_bin_dir" "$guard_target_root" "$build_tmp_root" "$cargo_home" \
  "$mise_data" "$mise_cache" "$rustup_home"
chmod 700 "$guard_state" "$guard_bin_dir" "$build_root" "$guard_target_root" \
  "$build_tmp_root" "$cargo_home" "$mise_cache" "$rustup_home"
check_directory "$state_root"
check_directory "$guard_state"
check_directory "$guard_bin_dir"
check_directory "$home_root"
check_directory "$home_root/.local"
check_directory "$home_root/.local/share"
check_directory "$cache_root"
check_directory "$cache_root/velnor"
check_directory "$build_root"
check_directory "$guard_target_root"
check_directory "$build_tmp_root"
check_directory "$cargo_home"
check_directory "$mise_data"
check_directory "$mise_cache"
check_directory "$rustup_home"
for config in "$cargo_home/config" "$cargo_home/config.toml"; do
  if [[ -L "$config" || -e "$config" ]]; then
    fail "isolated Cargo configuration is forbidden: $config"
  fi
done
run_tmp="$(mktemp -d "$build_tmp_root/archive-guard.XXXXXX")"
chmod 700 "$run_tmp"
check_directory "$run_tmp"
trap cleanup EXIT

run_mise() {
  env -i \
    HOME="$HOME" \
    PATH="$(dirname -- "$mise_bin"):/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    MISE_DATA_DIR="$mise_data" \
    MISE_CACHE_DIR="$mise_cache" \
    MISE_CARGO_HOME="$cargo_home" \
    CARGO_HOME="$cargo_home" \
    CARGO_TARGET_DIR="$guard_target" \
    RUSTUP_HOME="$rustup_home" \
    TMPDIR="$run_tmp" \
    "$mise_bin" --no-config --no-env --no-hooks "$@"
}

validate_local_cargo_closure() {
  local package package_path tree_status
  temporary="$(mktemp "$run_tmp/archive-guard-cargo-tree.XXXXXX")"
  if run_mise exec rust@1.98.1 -- cargo tree --locked \
    --manifest-path "$repository/Cargo.toml" -p velnor-archive-guard \
    --edges normal,build --prefix none --format '{p}' >"$temporary"; then
    tree_status=0
  else
    tree_status=$?
  fi
  while IFS= read -r package; do
    case "$package" in
      *" ("*")")
        package_path="${package##* (}"
        package_path="${package_path%)}"
        # Cargo annotates registry packages with labels such as
        # "(proc-macro)" and repeated nodes with "(*)". Accept only those
        # known non-path annotations; any unfamiliar form fails closed.
        case "$package_path" in
          /*)
            case "$package_path" in
              "$repository/crates/tools/velnor-archive-guard") ;;
              *) fail "local Cargo dependency is outside the source closure: $package_path" ;;
            esac
            ;;
          proc-macro|\*) ;;
          *)
            [[ "$package" == *" v"* && "$package" != *" ("* \
              && "$package" != *")"* ]] \
              || fail "unrecognized Cargo package identity: $package"
            ;;
        esac
        ;;
      *)
        [[ "$package" == *" v"* && "$package" != *" ("* \
          && "$package" != *")"* ]] \
          || fail "unrecognized Cargo package identity: $package"
        ;;
    esac
  done <"$temporary"
  rm -f -- "$temporary"
  temporary=''
  if ((tree_status != 0)); then
    fail 'cannot determine locked local Cargo dependency closure'
  fi
}

cd -- "$repository"
expected_mise_version="$(< "$repository/.mise-version")"
policy_mise_version="$(awk '
  /^\[tools\]$/ { in_tools = 1; next }
  /^\[/ { in_tools = 0 }
  in_tools && $1 == "mise" && $2 == "=" {
    value = $3
    gsub(/\"/, "", value)
    print value
    count++
  }
  END { if (count != 1) exit 1 }
' "$repository/.velnor/version-policy.toml")" \
  || fail 'cannot read the version-policy Mise pin'
[[ "$expected_mise_version" == "$policy_mise_version" ]] \
  || fail 'Mise pin differs between .mise-version and version-policy.toml'
mise_version="$(run_mise version)"
actual_mise_version="${mise_version%% *}"
[[ "$actual_mise_version" == "$expected_mise_version" ]] \
  || fail "Mise $actual_mise_version does not match pinned $expected_mise_version"
run_mise install rust@1.98.1
rustc_version="$(run_mise exec rust@1.98.1 -- rustc --version)"
cargo_version="$(run_mise exec rust@1.98.1 -- cargo --version)"
[[ "$rustc_version" == 'rustc 1.98.1 '* ]] || fail "unexpected compiler: $rustc_version"
[[ "$cargo_version" == 'cargo 1.98.1 '* ]] || fail "unexpected Cargo: $cargo_version"
source_fingerprint="$(archive_guard_source_fingerprint)"
target_identity="$(archive_guard_target_identity)"
guard_target="$guard_target_root/$source_fingerprint-$target_identity"
[[ ! -L "$guard_target" ]] || fail 'archive guard target namespace is a symlink'
mkdir -p -- "$guard_target"
chmod 700 "$guard_target"
check_directory "$guard_target"
for profile in "$guard_target/debug" "$guard_target/release"; do
  if [[ -e "$profile" || -L "$profile" ]]; then
    check_directory "$profile"
  fi
done
validate_local_cargo_closure
run_mise exec rust@1.98.1 -- cargo build --release --locked \
  --manifest-path "$repository/Cargo.toml" \
  -p velnor-archive-guard --bin velnor-archive-guard
guard_cargo_output="$guard_target/release/velnor-archive-guard"
check_directory "$guard_target/release"
check_cargo_output "$guard_cargo_output"
archive_guard_check_fingerprint "$guard_cargo_output" "$source_fingerprint"
after_build_fingerprint="$(archive_guard_source_fingerprint)"
[[ "$after_build_fingerprint" == "$source_fingerprint" ]] \
  || fail 'archive guard sources changed during compilation'
guard_executable="$guard_bin_dir/velnor-archive-guard"
temporary="$(mktemp "$guard_bin_dir/.archive-guard.XXXXXX")"
cp "$guard_cargo_output" "$temporary"
chmod 755 "$temporary"
check_executable "$temporary"
archive_guard_check_fingerprint "$temporary" "$source_fingerprint"
before_install_fingerprint="$(archive_guard_source_fingerprint)"
[[ "$before_install_fingerprint" == "$source_fingerprint" ]] \
  || fail 'archive guard sources changed before installation'
mv -f "$temporary" "$guard_executable"
temporary=''
check_executable "$guard_executable"
archive_guard_check_fingerprint "$guard_executable" "$source_fingerprint"
