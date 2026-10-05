//! Generator release: the canonical Linux and macOS `velnor-actions` assets.
//!
//! The tag is `generator-<sha>` with `--latest=false`. It does not move
//! `v0.1.0`. Attest jobs never receive `contents: write`. Only publish does.

use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;
use velnor_actions_contract::GeneratorReleaseSourceBinding;

use super::Schema2WorkflowRequest;
use super::features::{CHECKOUT_USES, base, finish, publish_step, run_step};

/// GitHub-hosted macOS label. The arm64 binary is not built on Ubuntu.
/// Same `jdx/mise-action` commit CI pins. Not a floating tag.
const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
/// Catalog version. Not a floating `latest`.
const MISE_VERSION: &str = "2026.9.18";
/// `actions/attest-build-provenance` tag `v4.2.2` (commit, not a floating tag).
const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
const LINUX_ARTIFACT: &str = "generator-linux-assets";
const MACOS_ARTIFACT: &str = "generator-macos-assets";
const MACOS_X86_64_ARTIFACT: &str = "generator-macos-x86_64-assets";
const LINUX_DIR: &str = "linux-assets";
const MACOS_DIR: &str = "macos-assets";
const MACOS_X86_64_DIR: &str = "macos-x86_64-assets";
const ASSET_DIR: &str = "assets";
const ACCEPTED_MANIFEST: &str =
    "${{ runner.temp }}/velnor-generator-accepted/velnor-actions-release-manifest.json";
const ACCEPTED_CHECKSUM: &str =
    "${{ runner.temp }}/velnor-generator-accepted/velnor-actions-release-manifest.json.sha256";
const ACCEPTANCE_RECEIPT: &str =
    "${{ runner.temp }}/velnor-generator-accepted/velnor-actions-release-acceptance.json";

const PUBLISHER_WORKFLOW_REF: &str =
    "tailrocks/velnor-new/.github/workflows/generator-release.yml@refs/heads/main";

/// One native build and attestation per canonical target, then one publish.
///
/// # Errors
///
/// An illegal hosted or macOS label fails.
pub(super) fn generator_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let binding = GeneratorReleaseSourceBinding::for_current_workflow(&request.version)
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let mut jobs = Vec::new();
    for target in binding.targets() {
        jobs.extend(build::target_jobs(target, &binding)?);
    }
    jobs.push(publish_job(hosted, &binding));
    Ok(document(jobs))
}

fn publish_script(binding: &GeneratorReleaseSourceBinding) -> String {
    format!(
        "set -euo pipefail\nPYTHONDONTWRITEBYTECODE=1 python3 scripts/generator-release/publish_generator_release.py --version {}",
        shell_quote(binding.version())
    )
}

fn publish_job(hosted: Yaml, binding: &GeneratorReleaseSourceBinding) -> (String, Yaml) {
    let needs = binding
        .targets()
        .flat_map(|target| {
            let workflow = target_workflow(target);
            [workflow.build_job_id, workflow.attest_job_id]
        })
        .collect::<Vec<_>>();
    let downloads = binding
        .targets()
        .map(|target| {
            let workflow = target_workflow(target);
            download_step(
                workflow.download_step_name,
                workflow.artifact_name,
                workflow.artifact_dir,
            )
        })
        .collect::<Vec<_>>();
    let mut steps = vec![checkout_step(), mise_step()];
    steps.extend(downloads);
    steps.extend([
        publish_step(&publish_script(binding)),
        upload_accepted_metadata_step(),
    ]);
    finish(
        "publish-generator",
        with_needs(
            with_permissions(
                trusted_main_dispatch(base("Publish velnor-actions", hosted, 30)),
                publish_permissions(),
            ),
            &needs,
        ),
        steps,
    )
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TargetWorkflow {
    pub build_job_id: &'static str,
    pub build_job_name: &'static str,
    pub attest_job_id: &'static str,
    pub attest_job_name: &'static str,
    pub artifact_name: &'static str,
    pub artifact_dir: &'static str,
    pub download_step_name: &'static str,
}

pub(super) const fn target_workflow(
    target: velnor_actions_contract::GeneratorReleaseTarget,
) -> TargetWorkflow {
    use velnor_actions_contract::GeneratorReleaseTarget;

    match target {
        GeneratorReleaseTarget::LinuxX86_64 => TargetWorkflow {
            build_job_id: "build-linux",
            build_job_name: "Build Linux velnor-actions",
            attest_job_id: "attest-linux",
            attest_job_name: "Attest Linux velnor-actions",
            artifact_name: LINUX_ARTIFACT,
            artifact_dir: LINUX_DIR,
            download_step_name: "Download Linux assets",
        },
        GeneratorReleaseTarget::MacosArm64 => TargetWorkflow {
            build_job_id: "build-macos",
            build_job_name: "Build macOS arm64 velnor-actions",
            attest_job_id: "attest-macos",
            attest_job_name: "Attest macOS arm64 velnor-actions",
            artifact_name: MACOS_ARTIFACT,
            artifact_dir: MACOS_DIR,
            download_step_name: "Download macOS arm64 assets",
        },
        GeneratorReleaseTarget::MacosX86_64 => TargetWorkflow {
            build_job_id: "build-macos-x86_64",
            build_job_name: "Build macOS x86_64 velnor-actions",
            attest_job_id: "attest-macos-x86_64",
            attest_job_name: "Attest macOS x86_64 velnor-actions",
            artifact_name: MACOS_X86_64_ARTIFACT,
            artifact_dir: MACOS_X86_64_DIR,
            download_step_name: "Download macOS x86_64 assets",
        },
    }
}

fn mise_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Setup Mise")),
        ("uses".to_owned(), Yaml::str(MISE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("cache".to_owned(), Yaml::str("false")),
                ("env".to_owned(), Yaml::str("false")),
                ("install".to_owned(), Yaml::str("false")),
                ("version".to_owned(), Yaml::str(MISE_VERSION)),
            ]),
        ),
    ])
}

fn trusted_main_dispatch(mut fields: Vec<(String, Yaml)>) -> Vec<(String, Yaml)> {
    let condition = format!(
        "github.event_name == 'workflow_dispatch' && github.repository == 'tailrocks/velnor-new' && github.ref == 'refs/heads/main' && github.workflow_ref == '{PUBLISHER_WORKFLOW_REF}' && github.workflow_sha == github.sha"
    );
    fields.push(("if".to_owned(), Yaml::str(condition)));
    fields
}

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
            ]),
        ),
    ])
}

fn upload_step(name: &str, artifact: &str, files: &[&str]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(newline_list(files))),
                ("retention-days".to_owned(), Yaml::Int(1)),
            ]),
        ),
    ])
}

fn download_step(name: &str, artifact: &str, path: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(path)),
            ]),
        ),
    ])
}

fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest built artifacts")),
        ("uses".to_owned(), Yaml::str(ATTEST_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![("subject-path".to_owned(), Yaml::str(subjects))]),
        ),
    ])
}

fn with_permissions(mut fields: Vec<(String, Yaml)>, perms: Yaml) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), perms));
    fields
}

fn with_needs(mut fields: Vec<(String, Yaml)>, needs: &[&str]) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(needs.iter().copied().map(Yaml::str).collect()),
    ));
    fields
}

/// Build uploads a workflow artifact. That needs `actions: write`, not contents write.
fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

/// Attest job: `id-token`, no `contents: write`.
fn attest_permissions() -> Yaml {
    perm(&[
        ("actions", "read"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

/// Only the release-upload job may write repository contents.
fn publish_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "write")])
}

fn upload_accepted_metadata_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Upload verified release metadata"),
        ),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                (
                    "name".to_owned(),
                    Yaml::str("velnor-generator-accepted-${{ github.sha }}"),
                ),
                (
                    "path".to_owned(),
                    Yaml::str(newline_list(&[
                        ACCEPTED_MANIFEST,
                        ACCEPTED_CHECKSUM,
                        ACCEPTANCE_RECEIPT,
                    ])),
                ),
                ("retention-days".to_owned(), Yaml::Int(14)),
            ]),
        ),
    ])
}

fn perm(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn subject_list(files: &[&str]) -> String {
    files
        .iter()
        .map(|file| format!("{ASSET_DIR}/{file}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn newline_list(files: &[&str]) -> String {
    files.join("\n")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn document(jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Generator release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}

#[cfg(test)]
#[path = "schema2_generator_release_provenance_tests.rs"]
mod provenance_tests;

#[path = "schema2_generator_release_build.rs"]
mod build;
