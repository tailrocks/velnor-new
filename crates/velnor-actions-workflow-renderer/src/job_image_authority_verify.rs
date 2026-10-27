// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::Yaml;

use super::{VERIFY_MARKER, VERIFY_STEP_NAME};

const VERIFY_PROGRAM: &str = r#"import hashlib
import json
import os
import sys
sys.path.insert(0, sys.argv[1])
from pathlib import Path
import job_image_authority_identity as identity_module
import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
record_path = Path(sys.argv[2])
archive_path = Path(sys.argv[3])
closure_path = Path(sys.argv[4])
record = record_module.validate_record(record_module.load_record(record_path))
verified = oci_module.verify_oci_archive(archive_path)
hasher = hashlib.sha256()
archive_size = 0
with archive_path.open("rb") as archive_stream:
    while chunk := archive_stream.read(1048576):
        hasher.update(chunk)
        archive_size += len(chunk)
publish_module.check_record_binding(
    record,
    hasher.hexdigest(),
    archive_size,
    verified,
)
try:
    expected_run_id = int(os.environ["VELNOR_EXPECTED_RUN_ID"])
    expected_run_attempt = int(os.environ["VELNOR_EXPECTED_RUN_ATTEMPT"])
except ValueError as error:
    raise publish_module.AuthorityError("Candidate run identity is incorrect.") from error
identity = {
    "repository": os.environ["VELNOR_EXPECTED_REPOSITORY"],
    "event": os.environ["VELNOR_EXPECTED_EVENT"],
    "workflow_ref": os.environ["VELNOR_EXPECTED_WORKFLOW_REF"],
    "ref": os.environ["VELNOR_EXPECTED_REF"],
    "authority_commit": os.environ["VELNOR_EXPECTED_SOURCE"],
    "run_id": expected_run_id,
    "run_attempt": expected_run_attempt,
    "source": os.environ["VELNOR_EXPECTED_SOURCE"],
}
identity_module.validate_same_run_candidate(record, identity)
if closure_path.is_symlink() or not closure_path.is_file():
    raise publish_module.AuthorityError("Candidate source closure must be a regular file.")
if closure_path.stat().st_size > 1048576:
    raise publish_module.AuthorityError("Candidate source closure exceeds its size limit.")
try:
    closure = json.loads(closure_path.read_bytes())
except (UnicodeDecodeError, json.JSONDecodeError) as error:
    raise publish_module.AuthorityError("Candidate source closure has an incorrect format.") from error
if (
    not isinstance(closure, dict)
    or closure.get("schema") != "velnor.job-image-native-source-closure.v1"
):
    raise publish_module.AuthorityError("Candidate source closure has an incorrect schema.")
if closure.get("source_commit") != os.environ["VELNOR_EXPECTED_SOURCE"]:
    raise publish_module.AuthorityError("Candidate source closure is not the current run source.")
print("fixed authority body accepted the candidate bytes")
"#;

pub(crate) fn verify_candidate_step(archive: &str, record: &str, closure: &str) -> Yaml {
    let mut run = format!(
        "set -euo pipefail\nauthority=\"$VELNOR_IMAGE_AUTHORITY_DIR\"\narchive=\"{archive}\"\nrecord=\"{record}\"\nclosure=\"{closure}\"\npython3 - \"$authority\" \"$record\" \"$archive\" \"$closure\" <<'{VERIFY_MARKER}'\n"
    );
    run.push_str(VERIFY_PROGRAM);
    run.push_str(VERIFY_MARKER);
    run.push('\n');
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(VERIFY_STEP_NAME)),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "VELNOR_EXPECTED_SOURCE".to_owned(),
                    Yaml::str("${{ github.sha }}"),
                ),
                (
                    "VELNOR_EXPECTED_REF".to_owned(),
                    Yaml::str("${{ github.ref }}"),
                ),
                (
                    "VELNOR_EXPECTED_REPOSITORY".to_owned(),
                    Yaml::str("${{ github.repository }}"),
                ),
                (
                    "VELNOR_EXPECTED_EVENT".to_owned(),
                    Yaml::str("${{ github.event_name }}"),
                ),
                (
                    "VELNOR_EXPECTED_WORKFLOW_REF".to_owned(),
                    Yaml::str(
                        "${{ github.repository }}/.github/workflows/job-image-candidate.yml@${{ github.ref }}",
                    ),
                ),
                (
                    "VELNOR_EXPECTED_RUN_ID".to_owned(),
                    Yaml::str("${{ github.run_id }}"),
                ),
                (
                    "VELNOR_EXPECTED_RUN_ATTEMPT".to_owned(),
                    Yaml::str("${{ github.run_attempt }}"),
                ),
            ]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ])
}
