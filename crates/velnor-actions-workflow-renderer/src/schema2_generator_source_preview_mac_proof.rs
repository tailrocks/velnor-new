// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::{JobTimeout, ReleaseTarget};

use super::super::features::{base, finish};
use super::{ProductReleasePins, source_preview_runtime, workflow_steps as steps};

pub(super) const PATH: &str = ".github/workflows/source-preview-mac-proof.yml";

const INPUTS: &[&str] = &[
    "public_source",
    "public_run_id",
    "public_run_attempt",
    "producer_run_id",
    "producer_run_attempt",
    "producer_artifact_id",
    "publisher_artifact_id",
];
const OUTPUTS: &[&str] = &[
    "manifest_digest",
    "checksums_digest",
    "license_digest",
    "notice_digest",
];

pub(super) fn document(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    Ok(source_preview_runtime::reusable_document(
        "Verify source preview Mac proof",
        INPUTS,
        OUTPUTS,
        proof_job(pins)?,
    ))
}

fn proof_job(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let mut fields = steps::with_permissions(
        base(
            "Verify Mac proof",
            Yaml::str("ubuntu-26.04"),
            JobTimeout::PRODUCT_COMPILER,
        ),
        steps::perm(&[("actions", "read"), ("contents", "read")]),
    );
    if let Some((_, timeout)) = fields.iter_mut().find(|(key, _)| key == "timeout-minutes") {
        *timeout = Yaml::Int(140);
    }
    fields.push((
        "outputs".into(),
        Yaml::Map(
            OUTPUTS
                .iter()
                .map(|name| {
                    (
                        (*name).to_string(),
                        Yaml::str(format!("${{{{ steps.verify_mac.outputs.{name} }}}}")),
                    )
                })
                .collect(),
        ),
    ));
    let mut actions = super::product_setup_steps(pins, ReleaseTarget::LinuxX86_64)?;
    let mut bootstrap = steps::bash_step_with_env(
        "Verify protected runtime SDK",
        &source_preview_runtime::script(pins),
        vec![
            ("GH_TOKEN", "${{ github.token }}"),
            ("PUBLIC_SOURCE", "${{ inputs.public_source }}"),
            ("PUBLIC_RUN_ID", "${{ inputs.public_run_id }}"),
            ("PUBLIC_RUN_ATTEMPT", "${{ inputs.public_run_attempt }}"),
        ],
    );
    if let Yaml::Map(fields) = &mut bootstrap {
        fields.push(("id".into(), Yaml::str("verify")));
    }
    actions.push(bootstrap);
    actions.push(steps::download_step_by_id(
        "Download exact release assets",
        "${{ inputs.publisher_artifact_id }}",
        "${{ runner.temp }}/runner-family-release/assets",
    ));
    let mut run = steps::bash_step_with_env(
        "Verify production Mac qualification",
        &proof_script(pins),
        vec![
            ("GH_TOKEN", "${{ github.token }}"),
            (
                "VERIFIED_LINUX_SHA256",
                "${{ steps.verify.outputs.linux_sha256 }}",
            ),
            ("PRODUCER_RUN_ID", "${{ inputs.producer_run_id }}"),
            ("PRODUCER_RUN_ATTEMPT", "${{ inputs.producer_run_attempt }}"),
            ("PRODUCER_ARTIFACT_ID", "${{ inputs.producer_artifact_id }}"),
            (
                "PUBLISHER_ARTIFACT_ID",
                "${{ inputs.publisher_artifact_id }}",
            ),
            ("VELNOR_Q_REPOSITORY", "${{ job.workflow_repository }}"),
            ("VELNOR_Q_PATH", "${{ job.workflow_file_path }}"),
            ("VELNOR_Q_REF", "${{ job.workflow_ref }}"),
            ("VELNOR_Q_SHA", "${{ job.workflow_sha }}"),
            ("VELNOR_Q_CHECK_RUN_ID", "${{ job.check_run_id }}"),
        ],
    );
    if let Yaml::Map(fields) = &mut run {
        fields.push(("id".into(), Yaml::str("verify_mac")));
    }
    actions.push(run);
    Ok(finish("verify", fields, actions))
}

fn proof_script(pins: &ProductReleasePins) -> String {
    let program = velnor_actions_mise::source_preview::SEALED_EXECUTOR;
    format!(
        "set -eu\nVELNOR_Q_GH=\"$(mise which gh --tool=gh@{})\"\nexport VELNOR_Q_GH\n/usr/bin/python3 -I -S - <<'PY'\n{program}\nPY",
        pins.gh_version
    )
}
