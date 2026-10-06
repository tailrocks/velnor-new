#!/bin/bash
set -eu
seed=/opt/velnor/seed
if [ -e "$seed/mise/KEY" ]; then
  echo "refusing to fill: mise KEY already exists" >&2
  exit 1
fi
mkdir -p "$seed/mise/tree" "$seed/rustup/tree" /tmp/velnor-cargo /tmp/mise-dl
curl -fsSL --proto '=https' --tlsv1.2 \
  -o /tmp/mise-dl/mise-v2026.9.18-linux-x64 \
  https://github.com/jdx/mise/releases/download/v2026.9.18/mise-v2026.9.18-linux-x64
echo "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4  /tmp/mise-dl/mise-v2026.9.18-linux-x64" | sha256sum -c -
install -m 0755 /tmp/mise-dl/mise-v2026.9.18-linux-x64 /usr/local/bin/mise
mise --version
export MISE_DATA_DIR="$seed/mise/tree"
export MISE_RUSTUP_HOME="$seed/rustup/tree"
export RUSTUP_HOME="$seed/rustup/tree"
export MISE_CARGO_HOME=/tmp/velnor-cargo
export CARGO_HOME=/tmp/velnor-cargo
mise --no-config --no-env --no-hooks install \
  rust@1.98.1 \
  mr-boxington@1.21.1 \
  actionlint@1.7.12 \
  shellcheck@0.11.0 \
  zizmor@1.30.1 \
  aqua:nextest-rs/nextest/cargo-nextest@0.9.146
mise --no-config --no-env --no-hooks exec rust@1.98.1 -- \
  rustup component add --toolchain 1.98.1-x86_64-unknown-linux-gnu clippy rustfmt
mise --no-config --no-env --no-hooks where 'mr-boxington@1.21.1'
mise --no-config --no-env --no-hooks where 'rust@1.98.1'
mbx_root="$(mise --no-config --no-env --no-hooks where 'mr-boxington@1.21.1')"
rust_root="$(mise --no-config --no-env --no-hooks where 'rust@1.98.1')"
test -x "$mbx_root/mbx"
test -x "$rust_root/rustc"
test "$("$mbx_root/mbx" --version)" = "mbx 1.21.1"
"$rust_root/rustc" +1.98.1 -vV | grep -Fqx 'release: 1.98.1'
test -d "$seed/rustup/tree/toolchains/1.98.1-x86_64-unknown-linux-gnu"
test -x "$seed/rustup/tree/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustc"
test -x "$seed/rustup/tree/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/cargo"
test -x "$seed/rustup/tree/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/clippy-driver" || \
  test -x "$seed/rustup/tree/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/cargo-clippy"
test -x "$seed/rustup/tree/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustfmt"
chmod -R a+rX "$seed/mise/tree" "$seed/rustup/tree"
test ! -e "$seed/mise/KEY"
echo SEED_TREES_OK
find "$seed/mise" "$seed/rustup" -maxdepth 3 \( -type d -o -type l -o -type f \) | sort
echo "--- symlinks ---"
find "$seed/mise/tree" "$seed/rustup/tree" -type l -ls | head -80
echo "--- du ---"
du -sh "$seed/mise/tree" "$seed/rustup/tree"
