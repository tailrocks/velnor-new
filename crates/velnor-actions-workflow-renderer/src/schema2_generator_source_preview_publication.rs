// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::{JobTimeout, ReleaseTarget};

use super::super::features::{base, finish};
use super::{ProductReleasePins, workflow_steps as steps};

const TAG_PUSH: &str = "${{ github.event_name == 'push' && github.repository == 'tailrocks/velnor-new' && startsWith(github.ref, 'refs/tags/sdk-preview-') }}";
const PAYLOAD: &str = "velnor-actions-x86_64-unknown-linux-gnu";
const RECEIPT: &str = "SDK_PREVIEW_RECEIPT.json";
const BUNDLE: &str = "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl";
const SOURCE_ID: &str = "${{ needs.source-preview-source.outputs.source_artifact_id }}";
const BUILD_ID: &str = "${{ needs.source-preview-source.outputs.payload_artifact_id }}";
const RECEIPT_ID: &str = "${{ needs.source-preview-draft.outputs.artifact_id }}";
const BUNDLE_ID: &str = "${{ needs.source-preview-attest.outputs.artifact_id }}";

pub(super) fn jobs(pins: &ProductReleasePins) -> Result<Vec<(String, Yaml)>, RenderError> {
    Ok(vec![draft(), attest(), publish(pins)?])
}

fn fields(name: &str, needs: &[&str], permissions: Yaml, output: bool) -> Vec<(String, Yaml)> {
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
    if output {
        fields.push((
            "outputs".to_owned(),
            Yaml::Map(vec![(
                "artifact_id".to_owned(),
                Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
            )]),
        ));
    }
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

fn receipt() -> Yaml {
    steps::download_step_by_id(
        "Download exact preview receipt",
        RECEIPT_ID,
        "source-preview-receipt",
    )
}

fn script(mode: &str) -> String {
    let program = include_str!("schema2_generator_source_preview_publication.py");
    format!(
        "set -eu\nenv -i PATH=/usr/bin:/bin HOME=\"$HOME\" GH_TOKEN=\"$GH_TOKEN\" GITHUB_SHA=\"$GITHUB_SHA\" GITHUB_REPOSITORY=\"$GITHUB_REPOSITORY\" GITHUB_EVENT_NAME=\"$GITHUB_EVENT_NAME\" GITHUB_REF=\"$GITHUB_REF\" GITHUB_WORKFLOW_REF=\"$GITHUB_WORKFLOW_REF\" GITHUB_WORKFLOW_SHA=\"$GITHUB_WORKFLOW_SHA\" GITHUB_RUN_ID=\"$GITHUB_RUN_ID\" GITHUB_RUN_ATTEMPT=\"$GITHUB_RUN_ATTEMPT\" RUNNER_TEMP=\"$RUNNER_TEMP\" PREVIEW_GH=\"${{PREVIEW_GH:-}}\" PREVIEW_MODE='{mode}' /usr/bin/python3 -I -S - <<'PY'\nimport os, sys\nsys.path.insert(0, os.path.join(os.environ['RUNNER_TEMP'], 'source-preview-support'))\n{program}\nPY"
    )
}

fn draft() -> (String, Yaml) {
    let run = script("draft");
    finish(
        "source-preview-draft",
        fields(
            "Draft source preview",
            &["source-preview-source"],
            steps::perm(&[("contents", "write")]),
            true,
        ),
        vec![
            source(),
            payload(),
            steps::bash_step_with_env(
                "Create exact preview draft",
                &run,
                vec![("GH_TOKEN", "${{ github.token }}")],
            ),
            steps::upload_step_with_id(
                "upload",
                "Retain exact preview receipt",
                "source-preview-receipt",
                &["${{ runner.temp }}/SDK_PREVIEW_RECEIPT.json"],
            ),
        ],
    )
}

fn attest() -> (String, Yaml) {
    let mut action = steps::attest_step(&format!(
        "source-preview-receipt/{RECEIPT}\nsource-preview-payload/{PAYLOAD}"
    ));
    if let Yaml::Map(ref mut values) = action {
        values.push(("id".to_owned(), Yaml::str("attest")));
    }
    let copy = format!(
        "set -eu\ntest -f \"$BUNDLE_PATH\"\ntest ! -L \"$BUNDLE_PATH\"\ntest -s \"$BUNDLE_PATH\"\ntest ! -e \"$RUNNER_TEMP/{BUNDLE}\"\ntest ! -L \"$RUNNER_TEMP/{BUNDLE}\"\ninstall -m 0400 \"$BUNDLE_PATH\" \"$RUNNER_TEMP/{BUNDLE}\""
    );
    finish(
        "source-preview-attest",
        fields(
            "Attest source preview",
            &["source-preview-source", "source-preview-draft"],
            steps::attest_permissions(),
            true,
        ),
        vec![
            payload(),
            receipt(),
            action,
            steps::bash_step_with_env(
                "Retain exact preview bundle",
                &copy,
                vec![("BUNDLE_PATH", "${{ steps.attest.outputs.bundle-path }}")],
            ),
            steps::upload_step_with_id(
                "upload",
                "Retain exact preview bundle",
                "source-preview-bundle",
                &["${{ runner.temp }}/SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"],
            ),
        ],
    )
}

fn publish(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let bootstrap = include_str!("native_helper/source_preview_bootstrap.py");
    let verifier = include_str!("native_helper/source_preview_stage_verify.py");
    let run = format!(
        "set -eu\nmkdir -m 700 \"$RUNNER_TEMP/source-preview-support\"\ncat > \"$RUNNER_TEMP/source-preview-support/source_preview_bootstrap.py\" <<'BOOTSTRAP'\n{bootstrap}\nBOOTSTRAP\ncat > \"$RUNNER_TEMP/source-preview-support/source_preview_stage_verify.py\" <<'VERIFIER'\n{verifier}\nVERIFIER\nPREVIEW_GH=\"$(mise which gh --tool=gh@{})\"\nexport PREVIEW_GH\n{}",
        pins.gh_version,
        script("publish")
    );
    let job = fields(
        "Publish source preview",
        &[
            "source-preview-source",
            "source-preview-draft",
            "source-preview-attest",
        ],
        steps::publish_permissions(),
        false,
    );
    Ok(finish(
        "source-preview-publish",
        job,
        vec![
            source(),
            payload(),
            receipt(),
            steps::download_step_by_id(
                "Download exact preview bundle",
                BUNDLE_ID,
                "source-preview-bundle",
            ),
            steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
            steps::mise_install_step("Install pinned GitHub CLI", &pins.install_gh_argv)?,
            steps::bash_step_with_env(
                "Verify and publish preview",
                &run,
                vec![("GH_TOKEN", "${{ github.token }}")],
            ),
        ],
    ))
}
