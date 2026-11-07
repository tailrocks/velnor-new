// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::features::{base, finish};
use super::{ProductReleasePins, workflow_steps as steps};
use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::{JobTimeout, ReleaseTarget};

const TAG_PUSH: &str = "${{ github.event_name == 'push' && github.repository == 'tailrocks/velnor-new' && startsWith(github.ref, 'refs/tags/sdk-preview-') }}";
const SOURCE_ID: &str = "${{ needs.source-preview-draft.outputs.source_artifact_id }}";
const BUILD_ID: &str = "${{ needs.source-preview-draft.outputs.payload_artifact_id }}";
const FRESH_SOURCE_ID: &str = "${{ needs.source-preview-source.outputs.source_artifact_id }}";
const FRESH_BUILD_ID: &str = "${{ needs.source-preview-source.outputs.payload_artifact_id }}";
const PAIR_ID: &str = "${{ needs.source-preview-attest.outputs.pair_id }}";

pub(super) fn jobs(pins: &ProductReleasePins) -> Result<Vec<(String, Yaml)>, RenderError> {
    Ok(vec![draft(pins)?, attest(pins)?, publish(pins)?])
}

fn fields(name: &str, needs: &[&str], permissions: Yaml) -> Vec<(String, Yaml)> {
    let mut fields = steps::with_needs(
        steps::with_permissions(
            base(
                name,
                Yaml::str("ubuntu-26.04"),
                JobTimeout::PRODUCT_COMPILER,
            ),
            permissions,
        ),
        needs,
    );
    fields.push(("if".to_owned(), Yaml::str(TAG_PUSH)));
    fields.push((
        "environment".to_owned(),
        Yaml::Map(vec![(
            "name".to_owned(),
            Yaml::str("generator-source-preview"),
        )]),
    ));
    fields
}

fn source() -> Yaml {
    steps::download_step_by_id(
        "Download admitted source metadata",
        SOURCE_ID,
        "source-preview-metadata",
    )
}

fn payload() -> Yaml {
    steps::download_step_by_id(
        "Download exact qualified payload",
        BUILD_ID,
        "source-preview-payload",
    )
}

fn condition(mut step: Yaml, expression: &str) -> Yaml {
    if let Yaml::Map(ref mut fields) = step {
        fields.push(("if".to_owned(), Yaml::str(expression)));
    }
    step
}

fn identified(mut step: Yaml, id: &str) -> Yaml {
    if let Yaml::Map(ref mut fields) = step {
        fields.push(("id".to_owned(), Yaml::str(id)));
    }
    step
}

fn script(mode: &str) -> String {
    let program = include_str!("schema2_generator_source_preview_publication.py");
    let authority = include_str!("schema2_generator_source_preview_authority.py");
    format!(
        "set -eu\nenv -i PATH=/usr/bin:/bin HOME=\"$HOME\" GH_TOKEN=\"$GH_TOKEN\" GITHUB_SHA=\"$GITHUB_SHA\" GITHUB_REPOSITORY=\"$GITHUB_REPOSITORY\" GITHUB_EVENT_NAME=\"$GITHUB_EVENT_NAME\" GITHUB_REF=\"$GITHUB_REF\" GITHUB_WORKFLOW_REF=\"$GITHUB_WORKFLOW_REF\" GITHUB_WORKFLOW_SHA=\"$GITHUB_WORKFLOW_SHA\" GITHUB_RUN_ID=\"$GITHUB_RUN_ID\" GITHUB_RUN_ATTEMPT=\"$GITHUB_RUN_ATTEMPT\" RUNNER_TEMP=\"$RUNNER_TEMP\" GITHUB_OUTPUT=\"$GITHUB_OUTPUT\" SOURCE_ARTIFACT_ID=\"$SOURCE_ARTIFACT_ID\" PAYLOAD_ARTIFACT_ID=\"$PAYLOAD_ARTIFACT_ID\" AUTHORITY_REPOSITORY=\"$AUTHORITY_REPOSITORY\" AUTHORITY_PATH=\"$AUTHORITY_PATH\" AUTHORITY_REF=\"$AUTHORITY_REF\" AUTHORITY_SHA=\"$AUTHORITY_SHA\" SIGN=\"${{SIGN:-}}\" PAIR_ORIGIN=\"${{PAIR_ORIGIN:-}}\" PAIR_ID=\"${{PAIR_ID:-}}\" RECEIPT_SHA256=\"${{RECEIPT_SHA256:-}}\" BUNDLE_SHA256=\"${{BUNDLE_SHA256:-}}\" PREVIEW_GH=\"${{PREVIEW_GH:-}}\" PREVIEW_MODE='{mode}' /usr/bin/python3 -I -S - <<'PY'\nimport os, sys\nsys.path.insert(0, os.path.join(os.environ['RUNNER_TEMP'], 'source-preview-support'))\n{authority}\n{program}\nPY"
    )
}

fn env() -> Vec<(&'static str, &'static str)> {
    let mut values = super::source_preview::authority_env();
    values.extend([
        ("GH_TOKEN", "${{ github.token }}"),
        ("SOURCE_ARTIFACT_ID", SOURCE_ID),
        ("PAYLOAD_ARTIFACT_ID", BUILD_ID),
    ]);
    values
}

fn supported_script(pins: &ProductReleasePins, mode: &str) -> String {
    let bootstrap = include_str!("native_helper/source_preview_bootstrap.py");
    let verifier = include_str!("native_helper/source_preview_stage_verify.py");
    format!(
        "set -eu\nmkdir -m 700 \"$RUNNER_TEMP/source-preview-support\"\ncat > \"$RUNNER_TEMP/source-preview-support/source_preview_bootstrap.py\" <<'BOOTSTRAP'\n{bootstrap}\nBOOTSTRAP\ncat > \"$RUNNER_TEMP/source-preview-support/source_preview_stage_verify.py\" <<'VERIFIER'\n{verifier}\nVERIFIER\nPREVIEW_GH=\"$(mise which gh --tool=gh@{})\"\nexport PREVIEW_GH\n{}",
        pins.gh_version,
        script(mode)
    )
}

fn draft(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let mut job = fields(
        "Draft source preview",
        &["source-preview-source"],
        steps::perm(&[("actions", "read"), ("contents", "write")]),
    );
    job.push(("outputs".to_owned(), draft_outputs()));
    Ok(finish(
        "source-preview-draft",
        job,
        vec![
            steps::download_step_by_id(
                "Download admitted source metadata",
                FRESH_SOURCE_ID,
                "source-preview-metadata",
            ),
            steps::download_step_by_id(
                "Download exact qualified payload",
                FRESH_BUILD_ID,
                "source-preview-payload",
            ),
            steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
            steps::mise_install_step("Install pinned GitHub CLI", &pins.install_gh_argv)?,
            identified(
                steps::bash_step_with_env(
                    "Create exact preview draft",
                    &supported_script(pins, "draft"),
                    {
                        let mut values = super::source_preview::authority_env();
                        values.extend([
                            ("GH_TOKEN", "${{ github.token }}"),
                            ("SOURCE_ARTIFACT_ID", FRESH_SOURCE_ID),
                            ("PAYLOAD_ARTIFACT_ID", FRESH_BUILD_ID),
                        ]);
                        values
                    },
                ),
                "draft",
            ),
            steps::upload_step_with_id(
                "upload",
                "Retain draft decision",
                "source-preview-decision-${{ github.run_attempt }}",
                &["${{ runner.temp }}/source-preview-decision.json"],
            ),
        ],
    ))
}

fn attest(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let mut job = fields(
        "Attest source preview",
        &["source-preview-source", "source-preview-draft"],
        steps::perm(super::preview_permissions::preview_permissions(
            super::preview_permissions::PreviewPermissionRole::Attest,
        )),
    );
    job.push(("outputs".to_owned(), attest_outputs()));
    let sign = "${{ steps.prepare.outputs.sign == 'true' }}";
    let action = condition(
        identified(
            steps::attest_step(
                "${{ runner.temp }}/SDK_PREVIEW_RECEIPT.json\nsource-preview-payload/velnor-actions-x86_64-unknown-linux-gnu",
            ),
            "attest",
        ),
        sign,
    );
    let copy = "set -eu\ntest -f \"$BUNDLE_PATH\"\ntest ! -L \"$BUNDLE_PATH\"\ntest -s \"$BUNDLE_PATH\"\ntest ! -e \"$RUNNER_TEMP/SDK_PREVIEW_RECEIPT.json.sigstore.jsonl\"\ninstall -m 0400 \"$BUNDLE_PATH\" \"$RUNNER_TEMP/SDK_PREVIEW_RECEIPT.json.sigstore.jsonl\"";
    Ok(finish(
        "source-preview-attest",
        job,
        vec![
            source(),
            payload(),
            steps::download_step_by_id(
                "Download draft decision",
                "${{ needs.source-preview-draft.outputs.artifact_id }}",
                "source-preview-decision",
            ),
            condition(
                steps::download_step_by_id(
                    "Download retained signed pair",
                    "${{ needs.source-preview-draft.outputs.pair_id }}",
                    "source-preview-signed",
                ),
                "${{ needs.source-preview-draft.outputs.sign == 'false' && needs.source-preview-draft.outputs.pair_origin == 'artifact' }}",
            ),
            steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
            steps::mise_install_step("Install pinned GitHub CLI", &pins.install_gh_argv)?,
            identified(
                steps::bash_step_with_env(
                    "Select or finalize signed candidate",
                    &supported_script(pins, "prepare"),
                    attest_variables(),
                ),
                "prepare",
            ),
            action,
            condition(
                steps::bash_step_with_env(
                    "Retain exact preview bundle",
                    copy,
                    vec![("BUNDLE_PATH", "${{ steps.attest.outputs.bundle-path }}")],
                ),
                sign,
            ),
            condition(
                steps::upload_step_with_id(
                    "upload",
                    "Retain exact signed pair",
                    "source-preview-signed-${{ github.run_attempt }}",
                    &[
                        "${{ runner.temp }}/SDK_PREVIEW_RECEIPT.json",
                        "${{ runner.temp }}/SDK_PREVIEW_RECEIPT.json.sigstore.jsonl",
                    ],
                ),
                sign,
            ),
        ],
    ))
}

fn publish(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let job = fields(
        "Publish source preview",
        &[
            "source-preview-source",
            "source-preview-draft",
            "source-preview-attest",
        ],
        steps::perm(super::preview_permissions::preview_permissions(
            super::preview_permissions::PreviewPermissionRole::Publish,
        )),
    );
    let mut variables = env();
    variables.extend([
        (
            "PAIR_ORIGIN",
            "${{ needs.source-preview-attest.outputs.pair_origin }}",
        ),
        ("PAIR_ID", PAIR_ID),
        (
            "RECEIPT_SHA256",
            "${{ needs.source-preview-attest.outputs.receipt_sha256 }}",
        ),
        (
            "BUNDLE_SHA256",
            "${{ needs.source-preview-attest.outputs.bundle_sha256 }}",
        ),
    ]);
    Ok(finish(
        "source-preview-publish",
        job,
        vec![
            source(),
            payload(),
            condition(
                steps::download_step_by_id(
                    "Download exact signed pair",
                    PAIR_ID,
                    "source-preview-signed",
                ),
                "${{ needs.source-preview-attest.outputs.pair_origin == 'artifact' }}",
            ),
            steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
            steps::mise_install_step("Install pinned GitHub CLI", &pins.install_gh_argv)?,
            steps::bash_step_with_env(
                "Verify and publish preview",
                &supported_script(pins, "publish"),
                variables,
            ),
        ],
    ))
}

fn draft_outputs() -> Yaml {
    Yaml::Map(vec![
        (
            "artifact_id".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
        ),
        (
            "source_artifact_id".to_owned(),
            Yaml::str("${{ steps.draft.outputs.source_artifact_id }}"),
        ),
        (
            "payload_artifact_id".to_owned(),
            Yaml::str("${{ steps.draft.outputs.payload_artifact_id }}"),
        ),
        (
            "sign".to_owned(),
            Yaml::str("${{ steps.draft.outputs.sign }}"),
        ),
        (
            "pair_origin".to_owned(),
            Yaml::str("${{ steps.draft.outputs.pair_origin }}"),
        ),
        (
            "pair_id".to_owned(),
            Yaml::str("${{ steps.draft.outputs.pair_id }}"),
        ),
        (
            "receipt_sha256".to_owned(),
            Yaml::str("${{ steps.draft.outputs.receipt_sha256 }}"),
        ),
        (
            "bundle_sha256".to_owned(),
            Yaml::str("${{ steps.draft.outputs.bundle_sha256 }}"),
        ),
    ])
}

fn attest_outputs() -> Yaml {
    Yaml::Map(vec![
        (
            "pair_origin".to_owned(),
            Yaml::str("${{ steps.prepare.outputs.pair_origin || 'artifact' }}"),
        ),
        (
            "pair_id".to_owned(),
            Yaml::str("${{ steps.prepare.outputs.pair_id || steps.upload.outputs.artifact-id }}"),
        ),
        (
            "receipt_sha256".to_owned(),
            Yaml::str("${{ steps.prepare.outputs.receipt_sha256 }}"),
        ),
        (
            "bundle_sha256".to_owned(),
            Yaml::str("${{ steps.prepare.outputs.bundle_sha256 }}"),
        ),
    ])
}

fn attest_variables() -> Vec<(&'static str, &'static str)> {
    let mut variables = env();
    variables.extend([
        ("SIGN", "${{ needs.source-preview-draft.outputs.sign }}"),
        (
            "PAIR_ORIGIN",
            "${{ needs.source-preview-draft.outputs.pair_origin }}",
        ),
        (
            "PAIR_ID",
            "${{ needs.source-preview-draft.outputs.pair_id }}",
        ),
        (
            "RECEIPT_SHA256",
            "${{ needs.source-preview-draft.outputs.receipt_sha256 }}",
        ),
        (
            "BUNDLE_SHA256",
            "${{ needs.source-preview-draft.outputs.bundle_sha256 }}",
        ),
    ]);
    variables
}
