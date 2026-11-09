// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Yaml, action, run, steps};

pub(super) fn steps() -> Vec<Yaml> {
    let mut context = run(
        "Record publication context",
        r#"set -euo pipefail
python3 - <<'PY_CONTEXT'
import hashlib, json, os
from pathlib import Path

def number(name):
    value = os.environ[name]
    if not value.isascii() or not value.isdecimal() or value.startswith("0") or int(value) > 2**64 - 1:
        raise ValueError(name)
    return int(value)

def digest(path):
    return "sha256:" + hashlib.sha256(Path(path).read_bytes()).hexdigest()

artifact_digest = os.environ["PUBLISHER_DIGEST"]
if len(artifact_digest) != 64 or any(c not in "0123456789abcdef" for c in artifact_digest):
    raise ValueError("publisher digest")
data = {
    "schema": "velnor.runner-image-publication-context.v1",
    "repository": os.environ["GITHUB_REPOSITORY"],
    "repository_id": number("GITHUB_REPOSITORY_ID"),
    "source_commit": os.environ["GITHUB_SHA"],
    "public_run_id": number("GITHUB_RUN_ID"),
    "public_run_attempt": number("GITHUB_RUN_ATTEMPT"),
    "producer_run_id": number("PRODUCER_RUN_ID"),
    "producer_run_attempt": number("PRODUCER_RUN_ATTEMPT"),
    "producer_artifact_id": number("PRODUCER_ARTIFACT_ID"),
    "publisher_artifact_id": number("PUBLISHER_ARTIFACT_ID"),
    "publisher_artifact_digest": "sha256:" + artifact_digest,
    "manifest_sha256": digest(os.environ["RUNNER_TEMP"] + "/runner-family-release/assets/RUNNER_IMAGE_MANIFEST.json"),
    "runner_lock_sha256": digest(os.environ["RUNNER_TEMP"] + "/runner-family-authenticated/runner.lock"),
}
if data["repository"] != "tailrocks/velnor-new" or data["repository_id"] != 1390620900 or len(data["source_commit"]) != 40 or any(c not in "0123456789abcdef" for c in data["source_commit"]):
    raise ValueError("source")
path = Path(os.environ["RUNNER_TEMP"]) / "runner-image-publication-context.json"
path.write_text(json.dumps(data, separators=(",", ":")) + "\n", encoding="utf-8")
PY_CONTEXT"#.to_owned(),
        &[
            ("PRODUCER_RUN_ID", "${{ needs.discover-producer.outputs.producer_run_id }}"),
            ("PRODUCER_RUN_ATTEMPT", "${{ needs.discover-producer.outputs.producer_run_attempt }}"),
            ("PRODUCER_ARTIFACT_ID", "${{ needs.discover-producer.outputs.producer_artifact_id }}"),
            ("PUBLISHER_ARTIFACT_ID", "${{ steps.assets.outputs.artifact-id }}"),
            ("PUBLISHER_DIGEST", "${{ steps.assets.outputs.artifact-digest }}"),
        ],
    );
    if let Yaml::Map(fields) = &mut context {
        fields.push((
            "if".into(),
            Yaml::str("${{ steps.reconcile.outputs.action == 'build' }}"),
        ));
    }
    let mut list = vec![context];
    let mut context_upload = action(
        "Upload publication context",
        steps::UPLOAD_ARTIFACT_USES,
        &[
            (
                "name",
                "runner-image-publication-context-${{ github.run_attempt }}",
            ),
            (
                "path",
                "${{ runner.temp }}/runner-image-publication-context.json",
            ),
            ("if-no-files-found", "error"),
        ],
    );
    if let Yaml::Map(fields) = &mut context_upload {
        fields.push(("id".into(), Yaml::str("context")));
        fields.push((
            "if".into(),
            Yaml::str("${{ steps.reconcile.outputs.action == 'build' }}"),
        ));
    }
    list.push(context_upload);
    list
}
