#!/bin/bash
set -eu
seed=/opt/velnor/seed
test ! -e "$seed/mise/KEY"
curl -fsSL --proto '=https' --tlsv1.2 \
  -o /tmp/mise-v2026.9.18-linux-x64 \
  https://github.com/jdx/mise/releases/download/v2026.9.18/mise-v2026.9.18-linux-x64
echo "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4  /tmp/mise-v2026.9.18-linux-x64" | sha256sum -c -
mkdir -p "$seed/mise/tree/bin"
install -m 0755 /tmp/mise-v2026.9.18-linux-x64 "$seed/mise/tree/bin/mise"
install -m 0755 /tmp/mise-v2026.9.18-linux-x64 /usr/local/bin/mise
for shim in "$seed/mise/tree/shims"/*; do
  ln -sfn ../bin/mise "$shim"
done
python3 - <<'PY'
import os
root = "/opt/velnor/seed/mise/tree"
for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
    for name in dirnames + filenames:
        path = os.path.join(dirpath, name)
        if not os.path.islink(path):
            continue
        target = os.readlink(path)
        if target.startswith(root + "/"):
            rel = os.path.relpath(target, os.path.dirname(path))
            os.remove(path)
            os.symlink(rel, path)
            print(f"relinked {path} -> {rel}")
PY
chmod -R a+rX "$seed/mise/tree" "$seed/rustup/tree"
python3 - <<'PY'
import os
root = "/opt/velnor/seed/mise/tree"
bad = []
for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
    for name in dirnames + filenames:
        path = os.path.join(dirpath, name)
        if os.path.islink(path):
            target = os.readlink(path)
            if target.startswith("/") or target.startswith("/tmp") or "/usr/local" in target:
                bad.append(f"{path} -> {target}")
if bad:
    raise SystemExit("absolute symlinks remain:\n" + "\n".join(bad))
print("relative-symlinks-ok")
PY
export RUSTUP_HOME="$seed/rustup/tree"
"$seed/mise/tree/rust-proxies/bin/rustc" +1.98.1 -vV | grep -Fqx 'release: 1.98.1'
# Fresh reader. Do not use the seed paths as RUSTUP_HOME.
home=/tmp/reader-home
tmp=/tmp/reader-tmp
rm -rf "$home" "$tmp"
mkdir -p "$home/.local/share/mise" "$tmp/velnor/rustup" "$tmp/velnor/cargo"
cp -R "$seed/mise/tree/." "$home/.local/share/mise/"
cp -R "$seed/rustup/tree/." "$tmp/velnor/rustup/"
chown -R runner:runner "$home" "$tmp"
runuser -u runner -- env \
  HOME="$home" \
  MISE_DATA_DIR="$home/.local/share/mise" \
  MISE_RUSTUP_HOME="$tmp/velnor/rustup" \
  RUSTUP_HOME="$tmp/velnor/rustup" \
  MISE_CARGO_HOME="$tmp/velnor/cargo" \
  CARGO_HOME="$tmp/velnor/cargo" \
  PATH="/usr/local/bin:/usr/bin:/bin" \
  bash -c '
set -eu
mise --version | grep -F "2026.9.18"
rust_root="$(mise --no-config --no-env --no-hooks where rust@1.98.1)"
mbx_root="$(mise --no-config --no-env --no-hooks where mr-boxington@1.21.1)"
echo "rust_root=$rust_root"
echo "mbx_root=$mbx_root"
case "$rust_root" in
  "$HOME"/*) ;;
  *) echo "rust root escaped the private copy" >&2; exit 1 ;;
esac
case "$mbx_root" in
  "$HOME"/*) ;;
  *) echo "mbx root escaped the private copy" >&2; exit 1 ;;
esac
test -x "$rust_root/rustc"
test -x "$mbx_root/mbx"
test "$("$mbx_root/mbx" --version)" = "mbx 1.21.1"
"$rust_root/rustc" +1.98.1 -vV | grep -Fqx "release: 1.98.1"
test -x "$RUSTUP_HOME/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/cargo-clippy"
test -x "$RUSTUP_HOME/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustfmt"
echo READER_OK
'
test ! -e "$seed/mise/KEY"
echo SEED_SIM_OK
