//! Permission and output fields for isolated release jobs.

use crate::yaml::Yaml;

use super::{
    GH_VERSION, ReleaseAssets, artifact_outputs, checkout_step, finish_job, job_base, mise_step,
    needs, run_env_step, run_step, with_runner,
};

pub(super) fn eligibility_job(runs_on: Yaml) -> (String, Yaml) {
    let mut fields = job_base(
        "Check exact release source",
        runs_on,
        70,
        &[("actions", "read"), ("contents", "read")],
    );
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![
            (
                "source_sha".to_owned(),
                Yaml::str("${{ steps.source.outputs.source_sha }}"),
            ),
            (
                "default_branch".to_owned(),
                Yaml::str("${{ steps.source.outputs.default_branch }}"),
            ),
            (
                "workflow_sha".to_owned(),
                Yaml::str("${{ steps.source.outputs.workflow_sha }}"),
            ),
            (
                "ci_run_id".to_owned(),
                Yaml::str("${{ steps.source.outputs.ci_run_id }}"),
            ),
            (
                "ci_attempt".to_owned(),
                Yaml::str("${{ steps.source.outputs.ci_attempt }}"),
            ),
        ]),
    ));
    finish_job(
        "release-eligibility",
        fields,
        vec![
            checkout_step(),
            mise_step(),
            run_step(
                "Install pinned GitHub CLI",
                &format!("mise --no-config --no-env --no-hooks install gh@{GH_VERSION}"),
            ),
            run_step(
                "Verify pinned GitHub CLI",
                &format!(
                    "set -euo pipefail\ntest \"$(mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh --version | head -n 1 | cut -d' ' -f3)\" = '{GH_VERSION}'"
                ),
            ),
            run_env_step(
                "Verify current default branch and Required CI",
                "source",
                "bash scripts/generator-release/check-source.sh",
                &[("GH_TOKEN", "${{ github.token }}")],
            ),
        ],
    )
}

pub(super) fn build_fields(name: &str, runs_on: Yaml, artifact_name: &str) -> Vec<(String, Yaml)> {
    let mut fields = job_base(
        name,
        runs_on,
        120,
        &[("actions", "write"), ("contents", "read")],
    );
    fields.extend(needs(&["release-eligibility"]));
    fields.push(("outputs".to_owned(), artifact_outputs("upload")));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![(
            "VELNOR_ARTIFACT_NAME".to_owned(),
            Yaml::str(artifact_name),
        )]),
    ));
    fields
}

pub(super) fn attest_fields(
    name: String,
    runs_on: Yaml,
    build_job: &str,
    artifact_name: &str,
) -> Vec<(String, Yaml)> {
    let mut fields = job_base(
        &name,
        runs_on,
        30,
        &[
            ("actions", "write"),
            ("artifact-metadata", "write"),
            ("attestations", "write"),
            ("contents", "read"),
            ("id-token", "write"),
        ],
    );
    fields.extend(needs(&["release-eligibility", build_job]));
    fields.push(("outputs".to_owned(), artifact_outputs("upload-bundle")));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![(
            "VELNOR_ARTIFACT_NAME".to_owned(),
            Yaml::str(artifact_name),
        )]),
    ));
    fields
}

pub(super) fn manifest_fields(artifact_name: &str) -> Vec<(String, Yaml)> {
    let mut fields = job_base(
        "Build canonical release manifest",
        Yaml::Null,
        20,
        &[("actions", "write"), ("contents", "read")],
    );
    fields.extend(needs(&[
        "release-eligibility",
        "build-linux",
        "build-macos",
    ]));
    fields.push(("outputs".to_owned(), artifact_outputs("upload-manifest")));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![(
            "VELNOR_ARTIFACT_NAME".to_owned(),
            Yaml::str(artifact_name),
        )]),
    ));
    fields
}

pub(super) fn manifest_attest_fields(artifact_name: &str) -> Vec<(String, Yaml)> {
    let mut fields = job_base(
        "Attest canonical release manifest",
        Yaml::Null,
        30,
        &[
            ("actions", "write"),
            ("artifact-metadata", "write"),
            ("attestations", "write"),
            ("contents", "read"),
            ("id-token", "write"),
        ],
    );
    fields.extend(needs(&["release-eligibility", "build-manifest"]));
    fields.push(("outputs".to_owned(), artifact_outputs("upload-bundle")));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![(
            "VELNOR_ARTIFACT_NAME".to_owned(),
            Yaml::str(artifact_name),
        )]),
    ));
    fields
}

pub(super) fn publish_fields(
    runs_on: Yaml,
    required: &[&str],
    assets: &ReleaseAssets,
) -> Vec<(String, Yaml)> {
    let mut fields = job_base(
        "Publish immutable Velnor Actions release",
        runs_on,
        80,
        &[("actions", "read"), ("contents", "write")],
    );
    fields.extend(needs(required));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![(
            "VELNOR_RELEASE_VERSION".to_owned(),
            Yaml::str(&assets.version),
        )]),
    ));
    fields
}

pub(super) fn publish_artifact_ids() -> Vec<String> {
    [
        "build-linux.outputs.artifact_id",
        "attest-linux.outputs.artifact_id",
        "build-macos.outputs.artifact_id",
        "attest-macos.outputs.artifact_id",
        "build-manifest.outputs.artifact_id",
        "attest-manifest.outputs.artifact_id",
    ]
    .iter()
    .map(|output| format!("${{{{ needs.{output} }}}}"))
    .collect()
}
