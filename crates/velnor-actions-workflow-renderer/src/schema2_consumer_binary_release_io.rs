//! Static operation environment and immutable artifact transfer steps.

use super::super::helpers::script_step;
use super::ConsumerBinaryReleaseSpec;
use crate::{
    RenderError,
    steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES},
    yaml::Yaml,
};

pub(super) fn operation(
    spec: &ConsumerBinaryReleaseSpec,
    op: &str,
    authenticated: bool,
    status_token: bool,
) -> Result<Yaml, RenderError> {
    let plan = String::from_utf8(
        spec.plan
            .canonical_bytes()
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?,
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let mut env = vec![
        ("VELNOR_RELEASE_PLAN", plan.as_str()),
        ("VELNOR_INTERNAL_OP", op),
        (
            "VELNOR_REQUEST_FILE",
            "${{ runner.temp }}/velnor/package-release-plan.json",
        ),
        (
            "VELNOR_RELEASE_CI_RUN_ID",
            "${{ needs.release-eligibility.outputs.ci_run_id }}",
        ),
        (
            "VELNOR_RELEASE_CI_RUN_ATTEMPT",
            "${{ needs.release-eligibility.outputs.ci_attempt }}",
        ),
    ];
    if authenticated {
        env.push(("GH_TOKEN", "${{ github.token }}"));
    }
    if status_token {
        env.push((
            "VELNOR_RELEASE_STATUS_TOKEN",
            "${{ secrets.IMMUTABILITY_READ_TOKEN }}",
        ));
    }
    let script = format!(
        "set -euo pipefail\numask 077\nmkdir -p \"$RUNNER_TEMP/velnor\"\n(set -C; printf '%s' \"$VELNOR_RELEASE_PLAN\" > \"$VELNOR_REQUEST_FILE\")\n\"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\"\n",
        spec.generator_version
    );
    Ok(script_step(
        "Run authenticated Velnor release operation",
        "release",
        &script,
        env,
    ))
}

pub(super) fn transfer_artifact(download: bool, kind: &str, directory: &str) -> Yaml {
    let name =
        format!("velnor-stable-{kind}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}");
    let fields = if download {
        vec![
            ("name".to_owned(), Yaml::str(name)),
            (
                "path".to_owned(),
                Yaml::str(format!("${{{{ runner.temp }}}}/velnor/{directory}")),
            ),
        ]
    } else {
        vec![
            ("name".to_owned(), Yaml::str(name)),
            (
                "path".to_owned(),
                Yaml::str(format!("${{{{ runner.temp }}}}/velnor/{directory}")),
            ),
            ("if-no-files-found".to_owned(), Yaml::str("error")),
            ("retention-days".to_owned(), Yaml::Int(30)),
            ("overwrite".to_owned(), Yaml::Bool(false)),
        ]
    };
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(if download {
                "Download exact stable release evidence"
            } else {
                "Retain stable release evidence"
            }),
        ),
        (
            "uses".to_owned(),
            Yaml::str(if download {
                DOWNLOAD_ARTIFACT_USES
            } else {
                UPLOAD_ARTIFACT_USES
            }),
        ),
        ("with".to_owned(), Yaml::Map(fields)),
    ])
}
