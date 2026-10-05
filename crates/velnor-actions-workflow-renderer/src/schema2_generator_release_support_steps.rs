//! Fixed GitHub Actions steps and permission maps for generator publication.

use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use super::assets::ReleaseAssets;

const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
const MISE_VERSION: &str = "2026.9.18";
const VERSION_GUARD: &str = "${{ github.event_name == 'workflow_dispatch' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.sha == github.workflow_sha }}";

pub(super) fn eligibility_job(runs_on: Yaml) -> (String, Yaml) {
    let mut fields = job_base(
        "Check exact release source",
        runs_on,
        20,
        &[ ("actions", "read"), ("contents", "read") ],
    );
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![
            ("source_sha".to_owned(), Yaml::str("${{ steps.source.outputs.source_sha }}")),
            ("default_branch".to_owned(), Yaml::str("${{ steps.source.outputs.default_branch }}")),
        ]),
    ));
    finish_job(
        "release-eligibility",
        fields,
        vec![
            checkout_step(),
            run_env_step(
                "Verify current default branch and Required CI",
                "source",
                "bash scripts/generator-release/check-source.sh",
                &[("GH_TOKEN", "${{ github.token }}")],
            ),
        ],
    )
}

pub(super) fn build_fields(
    name: &str,
    runs_on: Yaml,
    artifact_name: &str,
) -> Vec<(String, Yaml)> {
    let mut fields = job_base(name, runs_on, 120, &[]);
    fields.extend(needs(&["release-eligibility"]));
    fields.push(("outputs".to_owned(), artifact_outputs("upload")));
    fields.push(("env".to_owned(), Yaml::Map(vec![(
        "VELNOR_ARTIFACT_NAME".to_owned(),
        Yaml::str(artifact_name),
    )])));
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
            ("actions", "read"),
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
    let mut fields = job_base("Build canonical release manifest", Yaml::Null, 20, &[]);
    fields.extend(needs(&["release-eligibility", "build-linux", "build-macos"]));
    fields.push(("outputs".to_owned(), artifact_outputs("upload-manifest")));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![("VELNOR_ARTIFACT_NAME".to_owned(), Yaml::str(artifact_name))]),
    ));
    fields
}

pub(super) fn manifest_attest_fields(artifact_name: &str) -> Vec<(String, Yaml)> {
    let mut fields = job_base(
        "Attest canonical release manifest",
        Yaml::Null,
        30,
        &[
            ("actions", "read"),
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
        Yaml::Map(vec![("VELNOR_ARTIFACT_NAME".to_owned(), Yaml::str(artifact_name))]),
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
        40,
        &[("actions", "read"), ("contents", "write")],
    );
    fields.extend(needs(required));
    fields.push((
        "env".to_owned(),
        Yaml::Map(vec![("VELNOR_RELEASE_VERSION".to_owned(), Yaml::str(&assets.version))]),
    ));
    fields
}

pub(super) fn publish_artifact_ids(assets: &ReleaseAssets) -> Vec<String> {
    let _ = assets;
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
        ("name".to_owned(), Yaml::str("Set up pinned Mise without cache writes")),
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
    Yaml::Map(vec![("name".to_owned(), Yaml::str(name)), ("run".to_owned(), Yaml::str(run))])
}

fn run_env_step(name: &str, id: &str, run: &str, env: &[(&str, &str)]) -> Yaml {
    let mut step = vec![("name".to_owned(), Yaml::str(name)), ("id".to_owned(), Yaml::str(id))];
    step.push((
        "env".to_owned(),
        Yaml::Map(env.iter().map(|(key, value)| ((*key).to_owned(), Yaml::str(*value))).collect()),
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
                ("name".to_owned(), Yaml::str("${{ env.VELNOR_ARTIFACT_NAME }}")),
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
                ("repository".to_owned(), Yaml::str("${{ github.repository }}")),
                ("run-id".to_owned(), Yaml::str("${{ github.run_id }}")),
            ]),
        ),
    ])
}

pub(super) fn download_all_step(artifact_ids: &[String]) -> Yaml {
    let ids = artifact_ids.join(",");
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Download exact build and provenance artifacts")),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("artifact-ids".to_owned(), Yaml::str(ids)),
                ("digest-mismatch".to_owned(), Yaml::str("error")),
                ("merge-multiple".to_owned(), Yaml::str("true")),
                ("path".to_owned(), Yaml::str("assets")),
                ("repository".to_owned(), Yaml::str("${{ github.repository }}")),
                ("run-id".to_owned(), Yaml::str("${{ github.run_id }}")),
            ]),
        ),
    ])
}

pub(super) fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest exact downloaded files")),
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
        ("name".to_owned(), Yaml::str("Stage generated provenance bundle")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                ("BUNDLE_NAME".to_owned(), Yaml::str(bundle)),
                ("BUNDLE_PATH".to_owned(), Yaml::str("${{ steps.attest.outputs.bundle-path }}")),
            ]),
        ),
        (
            "run".to_owned(),
            Yaml::str("set -euo pipefail\ntest -n \"$BUNDLE_PATH\"\ntest -s \"$BUNDLE_PATH\"\ncp \"$BUNDLE_PATH\" \"$BUNDLE_NAME\""),
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
        ("VELNOR_REPOSITORY_ID".to_owned(), "${{ github.repository_id }}".to_owned()),
        ("VELNOR_BUILD_RUN_ID".to_owned(), "${{ github.run_id }}".to_owned()),
        ("VELNOR_BUILD_RUN_ATTEMPT".to_owned(), "${{ github.run_attempt }}".to_owned()),
    ];
    for (name, output) in [
        ("VELNOR_LINUX_ARTIFACT_ID", "build-linux.outputs.artifact_id"),
        ("VELNOR_LINUX_ARTIFACT_DIGEST", "build-linux.outputs.artifact_digest"),
        ("VELNOR_LINUX_BUNDLE_ID", "attest-linux.outputs.artifact_id"),
        ("VELNOR_LINUX_BUNDLE_DIGEST", "attest-linux.outputs.artifact_digest"),
        ("VELNOR_MACOS_ARTIFACT_ID", "build-macos.outputs.artifact_id"),
        ("VELNOR_MACOS_ARTIFACT_DIGEST", "build-macos.outputs.artifact_digest"),
        ("VELNOR_MACOS_BUNDLE_ID", "attest-macos.outputs.artifact_id"),
        ("VELNOR_MACOS_BUNDLE_DIGEST", "attest-macos.outputs.artifact_digest"),
        ("VELNOR_MANIFEST_ARTIFACT_ID", "build-manifest.outputs.artifact_id"),
        ("VELNOR_MANIFEST_ARTIFACT_DIGEST", "build-manifest.outputs.artifact_digest"),
        ("VELNOR_MANIFEST_BUNDLE_ID", "attest-manifest.outputs.artifact_id"),
        ("VELNOR_MANIFEST_BUNDLE_DIGEST", "attest-manifest.outputs.artifact_digest"),
    ] {
        env.push((name.to_owned(), format!("${{{{ needs.{output} }}}}")));
    }
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Verify provenance and publish immutable release")),
        (
            "env".to_owned(),
            Yaml::Map(env.into_iter().map(|(key, value)| (key, Yaml::str(value))).collect()),
        ),
        ("run".to_owned(), Yaml::str("bash scripts/generator-release/publish.sh")),
    ])
}

fn artifact_outputs(step_id: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "artifact_id".to_owned(),
            Yaml::str(format!("${{{{ steps.{step_id}.outputs.artifact-id }}}}")),
        ),
        (
            "artifact_digest".to_owned(),
            Yaml::str(format!("${{{{ steps.{step_id}.outputs.artifact-digest }}}}")),
        ),
    ])
}

fn job_base(
    name: &str,
    runs_on: Yaml,
    timeout: i64,
    pairs: &[(&str, &str)],
) -> Vec<(String, Yaml)> {
    vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), Yaml::Int(timeout)),
        ("if".to_owned(), Yaml::str(VERSION_GUARD)),
        ("permissions".to_owned(), permissions(pairs)),
    ]
}

fn permissions(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(pairs.iter().map(|(key, value)| ((*key).to_owned(), Yaml::str(*value))).collect())
}

fn needs(jobs: &[&str]) -> Vec<(String, Yaml)> {
    vec![("needs".to_owned(), Yaml::Seq(jobs.iter().map(|job| Yaml::str(*job)).collect()))]
}

pub(super) fn with_runner(mut fields: Vec<(String, Yaml)>, runs_on: Yaml) -> Vec<(String, Yaml)> {
    if let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "runs-on") {
        *value = runs_on;
    }
    fields
}

pub(super) fn finish_job(id: &str, mut fields: Vec<(String, Yaml)>, steps: Vec<Yaml>) -> (String, Yaml) {
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    (id.to_owned(), Yaml::Map(fields))
}
