// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::Yaml;

const RECORD: &str = include_str!("job_image_authority_record.py");
const OCI: &str = include_str!("job_image_authority_oci.py");
const TAR: &str = include_str!("job_image_authority_tar.py");

#[derive(Debug)]
struct AuthorityFile {
    path: &'static str,
    source: &'static str,
}

const FILES: [AuthorityFile; 3] = [
    AuthorityFile {
        path: "job_image_authority_record.py",
        source: RECORD,
    },
    AuthorityFile {
        path: "job_image_authority_oci.py",
        source: OCI,
    },
    AuthorityFile {
        path: "job_image_authority_tar.py",
        source: TAR,
    },
];

const DIRECTORY: &str = "$RUNNER_TEMP/velnor-image-authority";
const STEP_NAME: &str = "Materialize fixed image authority implementation";
const END_MARKER: &str = "VELNOR_IMAGE_AUTHORITY_EOF";
pub(super) const VERIFY_STEP_NAME: &str = "Verify candidate bytes with the fixed authority body";
pub(super) const VERIFY_MARKER: &str = "VELNOR_IMAGE_VERIFY_EOF";

pub(super) fn materialize_step() -> Yaml {
    let mut run = format!("set -eu\ndir=\"{DIRECTORY}\"\nrm -rf \"$dir\"\nmkdir -p \"$dir\"\n");
    for file in FILES {
        run = format!(
            "{run}cat > \"$dir/{}\" <<'{END_MARKER}'\n{}{END_MARKER}\n",
            file.path, file.source
        );
    }
    run.push_str("printf 'VELNOR_IMAGE_AUTHORITY_DIR=%s\\n' \"$dir\" >> \"$GITHUB_ENV\"\n");
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(STEP_NAME)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

#[path = "job_image_authority_verify.rs"]
mod verify;

pub(super) use verify::verify_candidate_step;
