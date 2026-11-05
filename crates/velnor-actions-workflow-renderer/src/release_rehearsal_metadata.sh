#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
unset GITHUB_TOKEN GH_TOKEN ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN
test "$(id -u)" -ne 0
[[ "$SOURCE_SHA" =~ ^[a-f0-9]{40}$ && "$APPROVED_SHA256" =~ ^[a-f0-9]{64}$ ]]
test "$(wc -c < "$RUNNER_TEMP/approved/approved-source.tar")" -le 104857600
printf '%s  %s\n' "$APPROVED_SHA256" "$RUNNER_TEMP/approved/approved-source.tar" | sha256sum -c -
python3 - "$RUNNER_TEMP/approved/approved-source.tar" "$RUNNER_TEMP/approved-tree" <<'PYTHON'
__VELNOR_EXTRACT_PYTHON__
PYTHON
mkdir "$RUNNER_TEMP/metadata-scratch"
cat > "$RUNNER_TEMP/metadata-runner.py" <<'PYTHON'
__VELNOR_METADATA_PYTHON__
PYTHON
check_source() {
  python3 - "$RUNNER_TEMP/metadata-runner.py" "$RUNNER_TEMP/approved-tree" "$RUNNER_TEMP/approved/approved-source.tar" <<'PYTHON'
import importlib.util
from pathlib import Path
import sys
spec = importlib.util.spec_from_file_location("metadata_runner", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.snapshot(Path(sys.argv[2]), Path(sys.argv[3]))
PYTHON
}
check_source
toolchain="$RUNNER_TEMP/velnor/rustup/toolchains/$RUST_VERSION-x86_64-unknown-linux-gnu"
test -x "$toolchain/bin/cargo" && test -x "$toolchain/bin/rustc"
docker run --rm --platform linux/amd64 --read-only --user "$(id -u):$(id -g)" \
  --cap-drop=ALL --security-opt=no-new-privileges --pids-limit=128 \
  --mount "type=bind,src=$RUNNER_TEMP/approved-tree,dst=/source,readonly" \
  --mount "type=bind,src=$RUNNER_TEMP/approved/approved-source.tar,dst=/source.tar,readonly" \
  --mount "type=bind,src=$toolchain,dst=/toolchain,readonly" \
  --mount "type=bind,src=$RUNNER_TEMP/metadata-runner.py,dst=/runner.py,readonly" \
  --mount "type=bind,src=$RUNNER_TEMP/metadata-scratch,dst=/scratch" \
  --entrypoint python3 'rust@sha256:4c4c8ec55be09785e6d6ac45ea9853525a73c15db74009100ac08952e773dfb8' \
  /runner.py /source /source.tar "$METADATA_MANIFEST" /toolchain/bin/cargo /toolchain/bin/rustc \
  /scratch/rustup /scratch /scratch/metadata.json
check_source
printf '%s  %s\n' "$APPROVED_SHA256" "$RUNNER_TEMP/approved/approved-source.tar" | sha256sum -c -
test ! -L "$RUNNER_TEMP/metadata-scratch/metadata.json"
test -f "$RUNNER_TEMP/metadata-scratch/metadata.json"
test "$(wc -c < "$RUNNER_TEMP/metadata-scratch/metadata.json")" -le 16777216
