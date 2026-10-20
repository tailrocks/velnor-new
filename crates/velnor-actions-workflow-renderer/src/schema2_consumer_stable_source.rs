//! Source-free stable selection: fresh dispatch or authenticated original-run rerun.
use super::shared::acquire;
use super::{ConsumerReleaseSpec, RenderError, Yaml, steps};
use crate::{
    commands::join_argv_for_run,
    schema2::features::{base, finish, run_step},
};
use velnor_actions_contract::JobTimeout;
pub(super) fn job(spec: &ConsumerReleaseSpec) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Authenticate stable source and complete CI",
        Yaml::str("ubuntu-26.04"),
        JobTimeout::DEFAULT,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str("${{ github.event_name == 'workflow_dispatch' }}"),
    ));
    fields.push((
        "permissions".to_owned(),
        steps::permissions(&[("contents", "read"), ("actions", "read")]),
    ));
    fields.push((
        "outputs".to_owned(),
        steps::outputs(&[
            "source_sha",
            "workflow_authority_sha",
            "ci_run_id",
            "ci_attempt",
            "commit_count",
        ]),
    ));
    let policy = String::from_utf8(
        spec.policy
            .canonical_bytes()
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?,
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let run = format!(
        "set -euo pipefail\numask 077\nmkdir -p \"$RUNNER_TEMP/velnor\"\n(set -C; printf '%s' \"$VELNOR_RELEASE_POLICY\" > \"$VELNOR_REQUEST_FILE\")\n\"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\"\n",
        spec.generator_version
    );
    let selection = steps::script(
        "Select only an authorized stable source",
        "source",
        &run,
        &[
            ("VELNOR_INTERNAL_OP", "select-stable-release-source-v1"),
            ("VELNOR_RELEASE_POLICY", &policy),
            (
                "VELNOR_REQUEST_FILE",
                "${{ runner.temp }}/velnor/package-release-policy.json",
            ),
            (
                "VELNOR_STABLE_VERSION_PACKAGE",
                &spec.policy.version.version_package,
            ),
            ("VELNOR_STABLE_OPERATION", "${{ inputs.operation }}"),
            (
                "VELNOR_STABLE_ORIGINAL_RUN_ID",
                "${{ inputs.original_run_id }}",
            ),
            (
                "VELNOR_STABLE_ORIGINAL_RUN_ATTEMPT",
                "${{ inputs.original_run_attempt }}",
            ),
            ("GH_TOKEN", "${{ github.token }}"),
        ],
    );
    Ok(finish(
        "stable-eligibility",
        fields,
        vec![
            steps::setup(&spec.linux_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            acquire(&spec.linux_acquire, "stable-eligibility", "ubuntu-26.04")?,
            selection,
        ],
    ))
}
