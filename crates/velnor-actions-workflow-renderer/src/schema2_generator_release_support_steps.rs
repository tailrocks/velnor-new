//! Fixed GitHub Actions steps and permission maps for generator publication.

use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use super::assets::ReleaseAssets;

#[path = "schema2_generator_release_support_job.rs"]
mod job;
#[path = "schema2_generator_release_support_roles.rs"]
mod roles;
use job::job_base;
pub(super) use job::{artifact_outputs, finish_job, needs, with_runner};

const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
const MISE_VERSION: &str = "2026.9.18";
const GH_VERSION: &str = "2.102.0";
const VERSION_GUARD: &str = "${{ github.event_name == 'workflow_dispatch' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.sha == github.workflow_sha }}";

pub(super) use roles::{
    attest_fields, build_fields, eligibility_job, manifest_attest_fields, manifest_fields,
    publish_artifact_ids, publish_fields,
};

pub(super) fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out exact event source")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
                ("token".to_owned(), Yaml::str("")),
            ]),
        ),
    ])
}

pub(super) fn mise_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Set up pinned Mise without cache writes"),
        ),
        ("uses".to_owned(), Yaml::str(MISE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("cache".to_owned(), Yaml::str("false")),
                ("cache_save".to_owned(), Yaml::str("false")),
                ("env".to_owned(), Yaml::str("false")),
                ("github_token".to_owned(), Yaml::str("")),
                ("install".to_owned(), Yaml::str("false")),
                ("version".to_owned(), Yaml::str(MISE_VERSION)),
            ]),
        ),
    ])
}

pub(super) fn run_step(name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

fn run_env_step(name: &str, id: &str, run: &str, env: &[(&str, &str)]) -> Yaml {
    let mut step = vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
    ];
    step.push((
        "env".to_owned(),
        Yaml::Map(
            env.iter()
                .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
                .collect(),
        ),
    ));
    step.push(("run".to_owned(), Yaml::str(run)));
    Yaml::Map(step)
}

pub(super) fn upload_step(id: &str, name: &str, files: &[&str]) -> Yaml {
    let path = files.join("\n");
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                (
                    "name".to_owned(),
                    Yaml::str("${{ env.VELNOR_ARTIFACT_NAME }}"),
                ),
                ("overwrite".to_owned(), Yaml::str("false")),
                ("path".to_owned(), Yaml::str(path)),
                ("retention-days".to_owned(), Yaml::Int(1)),
            ]),
        ),
    ])
}

pub(super) fn download_step(name: &str, artifact_id: &str, path: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("artifact-ids".to_owned(), Yaml::str(artifact_id)),
                ("digest-mismatch".to_owned(), Yaml::str("error")),
                ("path".to_owned(), Yaml::str(path)),
                (
                    "repository".to_owned(),
                    Yaml::str("${{ github.repository }}"),
                ),
                ("run-id".to_owned(), Yaml::str("${{ github.run_id }}")),
            ]),
        ),
    ])
}

pub(super) fn download_all_step(artifact_ids: &[String]) -> Yaml {
    let ids = artifact_ids.join(",");
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Download exact build and provenance artifacts"),
        ),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("artifact-ids".to_owned(), Yaml::str(ids)),
                ("digest-mismatch".to_owned(), Yaml::str("error")),
                ("merge-multiple".to_owned(), Yaml::str("true")),
                ("path".to_owned(), Yaml::str("assets")),
                (
                    "repository".to_owned(),
                    Yaml::str("${{ github.repository }}"),
                ),
                ("run-id".to_owned(), Yaml::str("${{ github.run_id }}")),
            ]),
        ),
    ])
}

pub(super) fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Attest exact downloaded files"),
        ),
        ("id".to_owned(), Yaml::str("attest")),
        ("uses".to_owned(), Yaml::str(ATTEST_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("github-token".to_owned(), Yaml::str("${{ github.token }}")),
                ("show-summary".to_owned(), Yaml::str("false")),
                ("subject-path".to_owned(), Yaml::str(subjects)),
            ]),
        ),
    ])
}

pub(super) fn copy_bundle_step(bundle: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Stage generated provenance bundle"),
        ),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                ("BUNDLE_NAME".to_owned(), Yaml::str(bundle)),
                (
                    "BUNDLE_PATH".to_owned(),
                    Yaml::str("${{ steps.attest.outputs.bundle-path }}"),
                ),
            ]),
        ),
        (
            "run".to_owned(),
            Yaml::str(
                "set -euo pipefail\ntest -n \"$BUNDLE_PATH\"\ntest -s \"$BUNDLE_PATH\"\ncp \"$BUNDLE_PATH\" \"$BUNDLE_NAME\"",
            ),
        ),
    ])
}

pub(super) fn manifest_step(version: &str) -> Yaml {
    run_env_step(
        "Create and validate canonical release manifest",
        "manifest",
        "bash scripts/generator-release/create-manifest.sh",
        &[
            ("VELNOR_RELEASE_VERSION", version),
            ("VELNOR_RELEASE_TAG", &format!("v{version}")),
        ],
    )
}

pub(super) fn publish_step(version: &str) -> Yaml {
    let mut env: Vec<(String, String)> = vec![
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        ("VELNOR_RELEASE_VERSION".to_owned(), version.to_owned()),
        ("VELNOR_RELEASE_TAG".to_owned(), format!("v{version}")),
        (
            "VELNOR_REPOSITORY_ID".to_owned(),
            "${{ github.repository_id }}".to_owned(),
        ),
        (
            "VELNOR_BUILD_RUN_ID".to_owned(),
            "${{ github.run_id }}".to_owned(),
        ),
        (
            "VELNOR_BUILD_RUN_ATTEMPT".to_owned(),
            "${{ github.run_attempt }}".to_owned(),
        ),
        (
            "VELNOR_EXPECTED_DEFAULT_BRANCH".to_owned(),
            "${{ needs.release-eligibility.outputs.default_branch }}".to_owned(),
        ),
        (
            "VELNOR_EXPECTED_WORKFLOW_SHA".to_owned(),
            "${{ needs.release-eligibility.outputs.workflow_sha }}".to_owned(),
        ),
    ];
    for (name, output) in [
        (
            "VELNOR_LINUX_ARTIFACT_ID",
            "build-linux.outputs.artifact_id",
        ),
        (
            "VELNOR_LINUX_ARTIFACT_DIGEST",
            "build-linux.outputs.artifact_digest",
        ),
        ("VELNOR_LINUX_BUNDLE_ID", "attest-linux.outputs.artifact_id"),
        (
            "VELNOR_LINUX_BUNDLE_DIGEST",
            "attest-linux.outputs.artifact_digest",
        ),
        (
            "VELNOR_MACOS_ARTIFACT_ID",
            "build-macos.outputs.artifact_id",
        ),
        (
            "VELNOR_MACOS_ARTIFACT_DIGEST",
            "build-macos.outputs.artifact_digest",
        ),
        ("VELNOR_MACOS_BUNDLE_ID", "attest-macos.outputs.artifact_id"),
        (
            "VELNOR_MACOS_BUNDLE_DIGEST",
            "attest-macos.outputs.artifact_digest",
        ),
        (
            "VELNOR_MANIFEST_ARTIFACT_ID",
            "build-manifest.outputs.artifact_id",
        ),
        (
            "VELNOR_MANIFEST_ARTIFACT_DIGEST",
            "build-manifest.outputs.artifact_digest",
        ),
        (
            "VELNOR_MANIFEST_BUNDLE_ID",
            "attest-manifest.outputs.artifact_id",
        ),
        (
            "VELNOR_MANIFEST_BUNDLE_DIGEST",
            "attest-manifest.outputs.artifact_digest",
        ),
    ] {
        env.push((name.to_owned(), format!("${{{{ needs.{output} }}}}")));
    }
    for (name, artifact_kind) in [
        ("VELNOR_LINUX_ARTIFACT_NAME", "linux-assets"),
        ("VELNOR_LINUX_BUNDLE_NAME", "linux-provenance"),
        ("VELNOR_MACOS_ARTIFACT_NAME", "macos-assets"),
        ("VELNOR_MACOS_BUNDLE_NAME", "macos-provenance"),
        ("VELNOR_MANIFEST_ARTIFACT_NAME", "manifest-assets"),
        ("VELNOR_MANIFEST_BUNDLE_NAME", "manifest-provenance"),
    ] {
        env.push((
            name.to_owned(),
            format!(
                "generator-{version}-{artifact_kind}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}"
            ),
        ));
    }
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify provenance and publish immutable release"),
        ),
        (
            "env".to_owned(),
            Yaml::Map(
                env.into_iter()
                    .map(|(key, value)| (key, Yaml::str(value)))
                    .collect(),
            ),
        ),
        (
            "run".to_owned(),
            Yaml::str("bash scripts/generator-release/publish.sh"),
        ),
    ])
}
