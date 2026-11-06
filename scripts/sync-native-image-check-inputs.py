#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import argparse
from pathlib import Path
import re

root = Path(__file__).resolve().parent.parent
config_path = root / ".velnor/config.toml"
inventory = root / "crates/velnor-actions-contract/src/image_candidate/closure_inventory.txt"
publication = root / "crates/velnor-actions-contract/src/image_candidate/runner_publication.rs"
paths = set(inventory.read_text().splitlines())
source = publication.read_text().split("pub const RUNNER_PUBLICATION_SOURCE_INPUTS:", 1)[1].split("];", 1)[0]
paths.update(re.findall(r'"([^"\n]+)"', source))
paths.update({"scripts/ci-native-image-candidates.sh", "scripts/ci-native-image-tool-build.sh", "scripts/ci-native-image-credential-exec.sh", "scripts/sync-native-image-check-inputs.py"})
for path in paths:
    candidate = root / path
    if not candidate.is_file() or candidate.is_symlink():
        raise SystemExit(f"invalid native image input: {path}")
block = '[[checks]]\nid = "native-image-candidates"\ntask = "validate-native-image-source"\n'
block += 'runner = { label = "ubuntu-26.04", platform = "linux_x64", executor = "hosted" }\n'
block += 'inputs = [\n' + ''.join(f'  "{path}",\n' for path in sorted(paths)) + ']\n'
block += 'tools = []\ntimeout_minutes = 90\n'
text = config_path.read_text()
marker = '[[checks]]\nid = "native-image-candidates"\n'
if marker in text:
    start = text.index(marker)
    end = text.find('\n[[', start + len(marker))
    end = len(text) if end < 0 else end + 1
    updated = text[:start] + block + text[end:]
else:
    updated = text.rstrip() + '\n\n' + block
parser = argparse.ArgumentParser()
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
if args.check:
    if updated != text:
        raise SystemExit('native image check inputs are stale')
else:
    config_path.write_text(updated)
