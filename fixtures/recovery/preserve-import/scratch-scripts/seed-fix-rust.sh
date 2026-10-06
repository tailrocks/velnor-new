#!/bin/bash
set -eu
seed=/opt/velnor/seed
tool="$seed/rustup/tree/toolchains/1.98.1-x86_64-unknown-linux-gnu"
test -x "$tool/bin/rustc"
test -x "$tool/bin/cargo"
test -x "$tool/bin/rustfmt"
test -x "$tool/bin/cargo-clippy"
test ! -e "$seed/mise/KEY"
if [ -L "$seed/mise/tree/installs/rust/1.98.1" ]; then
  rm "$seed/mise/tree/installs/rust/1.98.1"
fi
export RUSTUP_HOME="$seed/rustup/tree"
export CARGO_HOME="$seed/mise/tree/rust-proxies"
mkdir -p "$CARGO_HOME"
curl -fsSL --proto '=https' --tlsv1.2 https://sh.rustup.rs -o /tmp/rustup-init.sh
sh /tmp/rustup-init.sh -y --no-modify-path --default-toolchain 1.98.1
test -x "$CARGO_HOME/bin/rustc"
test -x "$CARGO_HOME/bin/cargo"
test -x "$CARGO_HOME/bin/rustup"
ln -s ../../rust-proxies/bin "$seed/mise/tree/installs/rust/1.98.1"
chmod -R a+rX "$seed/mise/tree" "$seed/rustup/tree"
# The job copy must not depend on this container's /tmp.
test ! -e /tmp/velnor-cargo || true
python3 - <<'PY'
import os
root = "/opt/velnor/seed/mise/tree"
bad = []
for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
    for name in dirnames + filenames:
        path = os.path.join(dirpath, name)
        if os.path.islink(path):
            target = os.readlink(path)
            if target.startswith("/tmp") or target.startswith("/usr/local"):
                bad.append(f"{path} -> {target}")
if bad:
    raise SystemExit("bad symlinks:\n" + "\n".join(bad))
print("symlink-scan-ok")
PY
echo "rust-link=$(readlink "$seed/mise/tree/installs/rust/1.98.1")"
"$CARGO_HOME/bin/rustc" +1.98.1 -vV | grep -Fqx 'release: 1.98.1'
echo RUST_PROXY_OK
