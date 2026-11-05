// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::workflow_steps;
use crate::yaml::Yaml;

pub(super) fn download_raw_step(name: &str, artifact_id: &str, path: &str) -> Yaml {
    let mut step = workflow_steps::download_step_by_id(name, artifact_id, path);
    if let Yaml::Map(fields) = &mut step {
        for (key, value) in fields {
            if key == "with" {
                if let Yaml::Map(inputs) = value {
                    inputs.push(("skip-decompress".to_owned(), Yaml::Bool(true)));
                }
                break;
            }
        }
    }
    step
}

pub(super) fn artifact_outputs(digest: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "artifact_id".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
        ),
        (
            "artifact_digest".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-digest }}"),
        ),
        (
            "artifact_size".to_owned(),
            Yaml::str("${{ steps.evidence_digest.outputs.artifact_size }}"),
        ),
        (
            digest.to_owned(),
            Yaml::str(format!("${{{{ steps.evidence_digest.outputs.{digest} }}}}")),
        ),
    ])
}

pub(super) fn evidence_steps(directory: &str, file: &str, digest: &str, hash: &str) -> Vec<Yaml> {
    let upload_path = if file == "admission" {
        format!("{directory}/admission.zip")
    } else {
        format!("{directory}/result.json")
    };
    let mut upload = workflow_steps::upload_step_with_id(
        "upload",
        "Upload bound evidence",
        directory,
        &[&upload_path],
    );
    if let Yaml::Map(fields) = &mut upload {
        for (key, value) in fields {
            if key == "with" {
                if let Yaml::Map(inputs) = value {
                    inputs.push(("archive".to_owned(), Yaml::Bool(false)));
                }
                break;
            }
        }
    }
    vec![
        workflow_steps::bash_step_with_id(
            "evidence_digest",
            "Bind evidence bytes",
            &format!(
                "set -eu\ntest -f {directory}/{file}.json && test ! -L {directory}/{file}.json\ndigest=\"$({hash} {directory}/{file}.json | awk 'NR == 1 {{print $1}}')\"\n[[ \"$digest\" =~ ^[0-9a-f]{{64}}$ ]]\nprintf '{digest}=%s\\n' \"$digest\" >> \"$GITHUB_OUTPUT\"\ntest -f {upload_path} && test ! -L {upload_path}\nsize=\"$(wc -c < {upload_path} | tr -d ' ')\"\n[[ \"$size\" =~ ^[1-9][0-9]*$ ]]\nprintf 'artifact_size=%s\\n' \"$size\" >> \"$GITHUB_OUTPUT\""
            ),
        ),
        upload,
    ]
}
