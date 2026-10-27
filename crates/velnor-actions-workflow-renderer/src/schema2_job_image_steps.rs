// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::steps::{CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use super::authority;
use super::native;
use super::{ARCHIVE, ARTIFACT_NAME, PUBLICATION_EVIDENCE, RECORD};

pub(super) fn gh_env() -> Yaml {
    // `gh` also reads GITHUB_TOKEN; GH_TOKEN stays out of the rendered jobs so
    // the data-only gates keep passing unchanged.
    Yaml::Map(vec![(
        "GITHUB_TOKEN".to_owned(),
        Yaml::str("${{ secrets.GITHUB_TOKEN }}"),
    )])
}

pub(super) fn fetch_ci_runs_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Fetch canonical CI runs for the candidate source"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("env".to_owned(), gh_env()),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "set -euo pipefail\ngh api \"repos/${{{{ github.repository }}}}/actions/workflows/ci.yml/runs?head_sha=${{{{ github.sha }}}}&per_page=100\" > \"{}\"",
                authority::CI_RUNS_PATH
            )),
        ),
    ])
}

pub(super) fn fetch_artifact_response_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Fetch candidate artifact record"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("env".to_owned(), gh_env()),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "set -euo pipefail\ngh api \"repos/${{{{ github.repository }}}}/actions/runs/${{{{ github.run_id }}}}/artifacts?per_page=100\" > \"{}\"",
                authority::ARTIFACT_RESPONSE_PATH
            )),
        ),
    ])
}

pub(super) fn fetch_source_inputs_step() -> Yaml {
    const RUN: &str = r#"set -euo pipefail
python3 - <<'VELNOR_SOURCE_INPUTS_EOF'
import base64
import hashlib
import json
import os
import re
import subprocess
repo = os.environ["VELNOR_SOURCE_REPO"]
sha = os.environ["VELNOR_SOURCE_SHA"]
out = os.environ["VELNOR_SOURCE_OUTPUT"]
paths = (
    ".velnor/runner.lock",
    "images/job/ubuntu-26.04/Dockerfile",
    "images/job/ubuntu-26.04/README.md",
    "images/job/ubuntu-26.04/build-test.sh",
    "images/job/ubuntu-26.04/candidate/candidate.py",
)
if not re.fullmatch(r"[0-9a-f]{40}", sha or ""):
    raise SystemExit("authenticated source commit is incorrect")
inputs = {}
for name in paths:
    raw = subprocess.check_output(
        ["gh", "api", f"repos/{repo}/contents/{name}?ref={sha}"],
        timeout=60,
    )
    if len(raw) > 1048576:
        raise SystemExit(f"source file response too large: {name}")
    payload = json.loads(raw)
    if payload.get("type") != "file" or payload.get("encoding") != "base64":
        raise SystemExit(f"source file is not a file: {name}")
    content = base64.b64decode("".join(payload["content"].split()))
    if len(content) > 1048576:
        raise SystemExit(f"source file too large: {name}")
    inputs[name] = hashlib.sha256(content).hexdigest()
with open(out, "w", encoding="utf-8") as stream:
    stream.write(json.dumps({"commit": sha, "inputs": inputs}, sort_keys=True) + "\n")
VELNOR_SOURCE_INPUTS_EOF"#;
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Fetch authenticated source inputs"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "GITHUB_TOKEN".to_owned(),
                    Yaml::str("${{ secrets.GITHUB_TOKEN }}"),
                ),
                (
                    "VELNOR_SOURCE_REPO".to_owned(),
                    Yaml::str("${{ github.repository }}"),
                ),
                (
                    "VELNOR_SOURCE_SHA".to_owned(),
                    Yaml::str("${{ github.sha }}"),
                ),
                (
                    "VELNOR_SOURCE_OUTPUT".to_owned(),
                    Yaml::str(authority::AUTHENTICATED_SOURCE_PATH),
                ),
            ]),
        ),
        ("run".to_owned(), Yaml::str(RUN)),
    ])
}

pub(super) fn verify_attestations_step(archive: &str, record: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify candidate attestations with Sigstore"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("env".to_owned(), gh_env()),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "set -euo pipefail\nrepo=\"${{{{ github.repository }}}}\"\nsha=\"${{{{ github.sha }}}}\"\nworkflow=\"$repo/.github/workflows/job-image-candidate.yml\"\ngh attestation verify \"{archive}\" --repo \"$repo\" --source-ref \"refs/heads/main\" --source-digest \"$sha\" --signer-workflow \"$workflow\" --signer-digest \"$sha\" --format json > \"{}\"\ngh attestation verify \"{record}\" --repo \"$repo\" --source-ref \"refs/heads/main\" --source-digest \"$sha\" --signer-workflow \"$workflow\" --signer-digest \"$sha\" --format json > \"{}\"",
                authority::ARCHIVE_ATTESTATION_PATH,
                authority::RECORD_ATTESTATION_PATH
            )),
        ),
    ])
}

pub(super) fn checkout_step(name: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
                ("fetch-depth".to_owned(), Yaml::Int(1)),
                ("persist-credentials".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

pub(super) fn run_step(name: &str, command: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(command)),
    ])
}

pub(super) fn upload_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Upload candidate archive and record"),
        ),
        ("id".to_owned(), Yaml::str("upload")),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(ARTIFACT_NAME)),
                (
                    "path".to_owned(),
                    Yaml::str(format!(
                        "${{{{ runner.temp }}}}/{ARCHIVE}\n${{{{ runner.temp }}}}/{RECORD}\n${{{{ runner.temp }}}}/{}",
                        native::CLOSURE
                    )),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(30)),
            ]),
        ),
    ])
}

pub(super) fn publication_evidence_upload_step(result: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Upload job image publication evidence"),
        ),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(PUBLICATION_EVIDENCE)),
                ("path".to_owned(), Yaml::str(result)),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(7)),
            ]),
        ),
    ])
}

pub(super) fn artifact_id_check_step(artifact_id: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Examine the uploaded artifact ID"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![("ARTIFACT_ID".to_owned(), Yaml::str(artifact_id))]),
        ),
        (
            "run".to_owned(),
            Yaml::str(
                "if [[ ! \"$ARTIFACT_ID\" =~ ^[1-9][0-9]*$ ]]; then\n  printf '%s\\n' 'The artifact ID is missing or incorrect.' >&2\n  exit 1\nfi",
            ),
        ),
    ])
}

pub(super) fn download_step(artifact_id: &str, destination: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Download the job image candidate from this workflow run"),
        ),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("artifact-ids".to_owned(), Yaml::str(artifact_id)),
                ("path".to_owned(), Yaml::str(destination)),
            ]),
        ),
    ])
}

pub(super) fn permissions(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}
