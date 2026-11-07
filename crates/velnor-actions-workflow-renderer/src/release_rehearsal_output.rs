// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{map, step};
use crate::yaml::Yaml;

#[derive(Clone, Copy)]
pub(super) enum OutputLocation {
    Produced,
    Transferred,
}

pub(super) fn validation_script(location: OutputLocation) -> String {
    let relative = match location {
        OutputLocation::Produced => "rehearsal-output.tar",
        OutputLocation::Transferred => "output/rehearsal-output.tar",
    };
    format!(
        "set -euo pipefail\np=\"$RUNNER_TEMP/{relative}\"\ntest ! -L \"$p\"\ntest -f \"$p\"\ntest -s \"$p\"\ntest \"$(wc -c < \"$p\")\" -le 104857600\n"
    )
}

pub(super) fn append_output_witness_steps(steps: &mut Vec<Yaml>) {
    steps.push(map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::DOWNLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("rehearsal-output-untrusted")),
                ("path", Yaml::str("${{ runner.temp }}/output")),
            ]),
        ),
    ]));
    steps.push(step(
        "Validate transferred rehearsal output",
        &validation_script(OutputLocation::Transferred),
        map(vec![]),
    ));
    steps.push(map(vec![
        ("id", Yaml::str("output_upload")),
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::UPLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("rehearsal-output-observation")),
                (
                    "path",
                    Yaml::str("${{ runner.temp }}/output/rehearsal-output.tar"),
                ),
                ("if-no-files-found", Yaml::str("error")),
                ("retention-days", Yaml::Int(1)),
            ]),
        ),
    ]));
    steps.push(step(
        "Bind rehearsal output artifact",
        &include_str!("release_rehearsal_artifact_marker.sh")
            .replace("VELNOR_REHEARSAL_WITNESS", "VELNOR_REHEARSAL_OUTPUT"),
        map(vec![
            (
                "VELNOR_ARTIFACT_ID",
                Yaml::str("${{ steps.output_upload.outputs.artifact-id }}"),
            ),
            (
                "VELNOR_ARTIFACT_DIGEST",
                Yaml::str("${{ steps.output_upload.outputs.artifact-digest }}"),
            ),
        ]),
    ));
}
