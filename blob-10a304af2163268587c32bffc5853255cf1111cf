//! Generic one-package consumer binary release workflow.

use crate::RenderError;
use crate::commands::join_argv_for_run;
use crate::marker::with_marker;
use crate::render::RenderedFile;
use crate::steps::{
    ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES,
    validate_uses,
};
use crate::yaml::{Yaml, render_yaml};

use super::MiseSetup;
use super::consumer_binary_release_scripts as scripts;
use super::consumer_release_eligibility::{self, ReleaseEligibilityContext};
use super::features::{base, finish, run_step};
use velnor_actions_contract::config::{CONSUMER_BINARY_TARGET, RustBinaryReleaseConfig};

#[path = "schema2_consumer_binary_release_helpers.rs"]
mod helpers;
use helpers::{
    checkout_step, download_artifact_step, identity_env, mise_step, needs, outputs, permissions,
    publish_job, script_step, upload_artifact_step, valid_repository,
};

/// Inputs resolved by the orchestrator for one consumer binary release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerBinaryReleaseSpec {
    /// Generator version embedded in the generated-file marker.
    pub generator_version: String,
    /// Consumer GitHub repository, derived from local `origin`.
    pub repository: String,
    /// Consumer's resolved default branch.
    pub default_branch: String,
    /// Repo-relative workspace-root `Cargo.toml`.
    pub manifest_path: String,
    /// Exact Cargo package name.
    pub package: String,
    /// Exact Cargo binary target name.
    pub bin: String,
    /// Pinned Mise setup for the Linux eligibility and publish jobs.
    pub linux_setup: MiseSetup,
    /// Pinned Mise setup for the Apple Silicon build job.
    pub macos_setup: MiseSetup,
    /// Complete Mise install argv for Rust and GitHub CLI.
    pub install_tools_argv: Vec<String>,
    /// Complete Mise install argv for Rust only.
    pub install_rust_argv: Vec<String>,
    /// Exact `mise exec ... cargo metadata --locked` argv.
    pub metadata_argv: Vec<String>,
    /// Exact `mise exec ... rustup target add` argv.
    pub add_target_argv: Vec<String>,
    /// Exact `mise exec ... cargo build --locked` argv.
    pub build_argv: Vec<String>,
    /// Exact pinned GitHub CLI argv prefix.
    pub gh_argv: Vec<String>,
}

/// The generated consumer binary workflow path.
pub(super) const BINARY_RELEASE_WORKFLOW: &str = ".github/workflows/binary-release.yml";

/// Render a marker-owned consumer workflow.
///
/// # Errors
///
/// Rejects invalid identities, pins, paths, commands, or generated YAML.
pub(super) fn render_consumer_binary_release(
    spec: &ConsumerBinaryReleaseSpec,
) -> Result<RenderedFile, RenderError> {
    validate_spec(spec)?;
    let metadata_command = join_argv_for_run(&spec.metadata_argv)?;
    let identity = scripts::identity(&metadata_command);
    let target_add = join_argv_for_run(&spec.add_target_argv)?;
    let build = join_argv_for_run(&spec.build_argv)?;
    let eligibility_context = ReleaseEligibilityContext {
        repository: spec.repository.clone(),
        default_branch: spec.default_branch.clone(),
        workflow_path: BINARY_RELEASE_WORKFLOW.to_owned(),
    };
    let eligibility =
        consumer_release_eligibility::consumer_script(&eligibility_context, &spec.gh_argv, false)?;
    let publisher_eligibility =
        consumer_release_eligibility::consumer_script(&eligibility_context, &spec.gh_argv, true)?;
    let asset = format!("{}-{CONSUMER_BINARY_TARGET}", spec.bin);
    let workflow = document(
        spec,
        &identity,
        &eligibility,
        &publisher_eligibility,
        &target_add,
        &build,
        &asset,
    )?;
    let bytes = with_marker(&spec.generator_version, &render_yaml(&workflow))?;
    Ok(RenderedFile {
        path: BINARY_RELEASE_WORKFLOW.to_owned(),
        bytes,
    })
}

fn validate_spec(spec: &ConsumerBinaryReleaseSpec) -> Result<(), RenderError> {
    let config = RustBinaryReleaseConfig {
        enabled: true,
        manifest_path: spec.manifest_path.clone(),
        package: spec.package.clone(),
        bin: spec.bin.clone(),
    };
    config
        .validate("consumer-config.toml")
        .map_err(|err| RenderError::InvalidWorkflow(err.to_string()))?;
    if !valid_repository(&spec.repository)
        || !velnor_actions_contract::is_valid_branch_name(&spec.default_branch)
        || spec.generator_version.is_empty()
        || !spec.manifest_path.ends_with("Cargo.toml")
    {
        return Err(RenderError::InvalidWorkflow(
            "binary_release_spec_invalid".to_owned(),
        ));
    }
    spec.linux_setup.validate()?;
    spec.macos_setup.validate()?;
    validate_uses(CHECKOUT_USES)?;
    validate_uses(UPLOAD_ARTIFACT_USES)?;
    validate_uses(DOWNLOAD_ARTIFACT_USES)?;
    validate_uses(ATTEST_BUILD_PROVENANCE_USES)?;
    for argv in [
        &spec.install_tools_argv,
        &spec.install_rust_argv,
        &spec.metadata_argv,
        &spec.add_target_argv,
        &spec.build_argv,
        &spec.gh_argv,
    ] {
        join_argv_for_run(argv)?;
    }
    Ok(())
}

fn document(
    spec: &ConsumerBinaryReleaseSpec,
    identity: &str,
    eligibility: &str,
    publisher_eligibility: &str,
    target_add: &str,
    build: &str,
    asset: &str,
) -> Result<Yaml, RenderError> {
    let jobs = vec![
        eligibility_job(spec, eligibility, identity)?,
        build_job(spec, identity, target_add, build, asset)?,
        attest_job(asset),
        publish_job(spec, publisher_eligibility, identity, asset)?,
    ];
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Rust binary release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "workflow_dispatch".to_owned(),
                Yaml::Map(Vec::new()),
            )]),
        ),
        ("permissions".to_owned(), Yaml::Map(Vec::new())),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("consumer-binary-release-${{ github.repository }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "defaults".to_owned(),
            Yaml::Map(vec![(
                "run".to_owned(),
                Yaml::Map(vec![("shell".to_owned(), Yaml::str("bash"))]),
            )]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]))
}

fn eligibility_job(
    spec: &ConsumerBinaryReleaseSpec,
    eligibility: &str,
    identity: &str,
) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Check source and Required CI",
        Yaml::str("ubuntu-26.04"),
        75,
    );
    fields.push((
        "permissions".to_owned(),
        permissions(&[("actions", "read"), ("contents", "read")]),
    ));
    fields.push((
        "outputs".to_owned(),
        outputs(&[
            ("source_sha", "steps.source.outputs.source_sha"),
            (
                "workflow_authority_sha",
                "steps.source.outputs.workflow_authority_sha",
            ),
            ("ci_run_id", "steps.source.outputs.ci_run_id"),
            ("ci_attempt", "steps.source.outputs.ci_attempt"),
            ("package_version", "steps.identity.outputs.package_version"),
            ("tag", "steps.identity.outputs.tag"),
        ]),
    ));
    Ok(finish(
        "release-eligibility",
        fields,
        vec![
            checkout_step("${{ github.sha }}"),
            mise_step(&spec.linux_setup),
            run_step(
                "Install pinned Rust and GitHub CLI",
                &join_argv_for_run(&spec.install_tools_argv)?,
            ),
            script_step(
                "Verify default branch and latest Required CI",
                "source",
                eligibility,
                vec![("GH_TOKEN", "${{ github.token }}")],
            ),
            script_step(
                "Bind one package, binary, and Cargo version",
                "identity",
                identity,
                identity_env(spec),
            ),
        ],
    ))
}

fn build_job(
    spec: &ConsumerBinaryReleaseSpec,
    identity: &str,
    target_add: &str,
    build: &str,
    asset: &str,
) -> Result<(String, Yaml), RenderError> {
    let mut fields = base("Build one Apple Silicon binary", Yaml::str("macos-15"), 120);
    fields.push(("needs".to_owned(), needs(&["release-eligibility"])));
    fields.push((
        "permissions".to_owned(),
        permissions(&[("contents", "read")]),
    ));
    let run = scripts::build(identity, target_add, build);
    let mut steps = vec![
        checkout_step("${{ needs.release-eligibility.outputs.source_sha }}"),
        mise_step(&spec.macos_setup),
        run_step(
            "Install pinned Rust",
            &join_argv_for_run(&spec.install_rust_argv)?,
        ),
        script_step(
            "Build and verify exact binary",
            "build",
            &run,
            vec![
                ("MANIFEST_PATH", spec.manifest_path.as_str()),
                ("PACKAGE_NAME", spec.package.as_str()),
                ("BINARY_NAME", spec.bin.as_str()),
                ("TARGET_TRIPLE", CONSUMER_BINARY_TARGET),
                (
                    "EXPECTED_VERSION",
                    "${{ needs.release-eligibility.outputs.package_version }}",
                ),
                (
                    "EXPECTED_TAG",
                    "${{ needs.release-eligibility.outputs.tag }}",
                ),
            ],
        ),
    ];
    steps.push(upload_artifact_step(asset));
    Ok(finish("build-binary", fields, steps))
}

fn attest_job(asset: &str) -> (String, Yaml) {
    let mut fields = base("Attest binary assets", Yaml::str("ubuntu-26.04"), 20);
    fields.push(("needs".to_owned(), needs(&["build-binary"])));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("artifact-metadata", "write"),
            ("attestations", "write"),
            ("contents", "read"),
            ("id-token", "write"),
        ]),
    ));
    finish(
        "attest-binary",
        fields,
        vec![
            download_artifact_step(),
            Yaml::Map(vec![
                (
                    "name".to_owned(),
                    Yaml::str("Attest immutable release subjects"),
                ),
                ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
                (
                    "with".to_owned(),
                    Yaml::Map(vec![(
                        "subject-path".to_owned(),
                        Yaml::str(format!(
                            "assets/{asset}\nassets/SHA256SUMS\nassets/release.json"
                        )),
                    )]),
                ),
            ]),
        ],
    )
}

#[cfg(test)]
#[path = "schema2_consumer_binary_release_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "schema2_consumer_binary_release_runtime_tests.rs"]
mod runtime_tests;
