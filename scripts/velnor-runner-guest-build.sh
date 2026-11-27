# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
exec 1>&2
checkout=$1 root=$2 key=$3 triple=$4 mise_version=$5 mise_sha256=$6
umask 077
mkdir -p "$root"
chmod 0700 "$root"
[ "$(stat -c %u:%a "$root")" = 0:700 ] || { echo "$root must be a real directory owned by root with mode 0700" >&2; exit 1; }
mkdir -p "$root"/{bin,home,data,cache,state,config,cargo,rustup,checkouts}
work=$root/checkouts/$key
src=$work/src
store=$work/mbx
output=$work/velnor-runner
mise=$root/bin/mise

exec 9>"$work.lock"
flock 9
cleanup() { rm -rf "$src" "$store" "$output.tmp"; }
trap cleanup EXIT
cleanup
mkdir -p "$src" "$store"
rsync -a --no-owner --no-group --files-from=- --from0 "$checkout/" "$src/"

digest() { sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }
if [ "$(digest "$mise")" != "$mise_sha256" ]; then
  rm -f "$mise.download"
  curl --proto '=https' --tlsv1.2 -fsSL -o "$mise.download" \
    "https://github.com/jdx/mise/releases/download/v$mise_version/mise-v$mise_version-linux-arm64"
  if [ "$(digest "$mise.download")" != "$mise_sha256" ]; then
    rm -f "$mise.download"
    echo "mise $mise_version: the downloaded binary does not match the pinned sha256 $mise_sha256" >&2
    exit 1
  fi
  chmod 755 "$mise.download"
  mv -f "$mise.download" "$mise"
fi
[ "$(digest "$mise")" = "$mise_sha256" ]

build() {
  env -i PATH="$root/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" HOME="$root/home" LANG=C.UTF-8 \
    MISE_DATA_DIR="$root/data" MISE_CACHE_DIR="$root/cache" MISE_STATE_DIR="$root/state" MISE_CONFIG_DIR="$root/config" \
    MISE_TRUSTED_CONFIG_PATHS="$src" CARGO_HOME="$root/cargo" RUSTUP_HOME="$root/rustup" MBX_CACHE_DIR="$store" "$@"
}

cd "$src"
build mise install --locked
build mise exec -- bash -c 'first=$(type -a cargo | sed -n 1p); [ "$first" = "cargo is $MISE_DATA_DIR/command-wrappers/bin/cargo" ] || { echo "cargo resolves to: $first" >&2; exit 1; }'
build mise exec -- mbx doctor
build mise exec -- cargo build --release --locked -p velnor-runner --target "$triple"
install -m 755 "$src/target/$triple/release/velnor-runner" "$output.tmp"
mv -f "$output.tmp" "$output"
