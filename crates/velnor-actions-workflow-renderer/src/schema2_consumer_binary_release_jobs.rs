//! Isolated jobs for stable raw-binary signing and publication.

use super::{
    super::features::{base, finish, run_step},
    ConsumerBinaryReleaseSpec,
    helpers::{
        checkout_step, identity_env, mise_step, needs, outputs, permissions, script_step,
        upload_artifact_step, valid_repository,
    },
};
#[path = "schema2_consumer_binary_release_io.rs"]
mod io;
use super::super::consumer_release_eligibility;
use super::scripts;
use crate::{
    RenderError,
    commands::join_argv_for_run,
    document_steps::non_internal_step_to_yaml,
    marker::with_marker,
    render::RenderedFile,
    steps::{
        ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES,
        validate_uses,
    },
    yaml::{Yaml, render_yaml},
};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    config::{CONSUMER_BINARY_TARGET, RustBinaryReleaseConfig},
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
};

/// The generated stable workflow path.
pub(super) const BINARY_RELEASE_WORKFLOW: &str = ".github/workflows/binary-release.yml";

/// Render the stable release workflow.
/// # Errors
/// Rejects invalid plan identities, pins, paths, and commands.
pub(super) fn render(spec: &ConsumerBinaryReleaseSpec) -> Result<RenderedFile, RenderError> {
    render_with_routing(
        spec,
        &crate::workflow_routing::WorkflowRoutingPolicy::Preserve,
    )
}

pub(super) fn render_with_routing(
    spec: &ConsumerBinaryReleaseSpec,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
) -> Result<RenderedFile, RenderError> {
    validate_spec(spec)?;
    let metadata_command = join_argv_for_run(&spec.metadata_argv)?;
    let identity = scripts::identity(&metadata_command);
    let target_add = join_argv_for_run(&spec.add_target_argv)?;
    let build = join_argv_for_run(&spec.build_argv)?;
    let context = consumer_release_eligibility::ReleaseEligibilityContext {
        repository: spec.repository.clone(),
        default_branch: spec.default_branch.clone(),
        channel: PackageReleaseChannel::Stable,
    };
    let eligibility =
        consumer_release_eligibility::consumer_script(&context, &spec.gh_argv, false)?;
    let asset = format!("{}-{CONSUMER_BINARY_TARGET}", spec.bin);
    let workflow = crate::workflow_routing::route_workflow_yaml(
        BINARY_RELEASE_WORKFLOW,
        document(spec, &identity, &eligibility, &target_add, &build, &asset)?,
        routing,
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
        preview: velnor_actions_contract::config::RustBinaryPreviewConfig::default(),
    };
    config
        .validate("consumer-config.toml")
        .map_err(|err| RenderError::InvalidWorkflow(err.to_string()))?;
    spec.plan
        .validate()
        .map_err(|err| RenderError::InvalidWorkflow(err.to_string()))?;
    if !valid_repository(&spec.repository)
        || !velnor_actions_contract::is_valid_branch_name(&spec.default_branch)
        || spec.generator_version.is_empty()
        || !spec.manifest_path.ends_with("Cargo.toml")
        || spec.plan.channel != PackageReleaseChannel::Stable
        || spec.plan.repository != spec.repository
        || spec.plan.default_branch != spec.default_branch
        || spec.plan.products.len() != 1
        || spec.plan.products[0].package != spec.package
        || spec.plan.products[0].id != spec.package
        || spec.plan.products[0].bins.len() != 1
        || spec.plan.products[0].bins[0] != spec.bin
        || spec.plan.products[0].targets.len() != 1
        || spec.plan.products[0].targets[0] != CONSUMER_BINARY_TARGET
        || spec.plan.products[0].format != PackageReleasePayloadFormat::RawBinary
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
        &spec.install_gh_argv,
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
    target_add: &str,
    build: &str,
    asset: &str,
) -> Result<Yaml, RenderError> {
    let jobs = vec![
        eligibility_job(spec, eligibility, identity)?,
        build_job(spec, identity, target_add, build, asset)?,
        attest_job(spec, asset)?,
        publish_job(spec)?,
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

fn attest_job(
    spec: &ConsumerBinaryReleaseSpec,
    asset: &str,
) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Independently validate and sign binary assets",
        Yaml::str("ubuntu-26.04"),
        30,
    );
    fields.push((
        "needs".to_owned(),
        needs(&["release-eligibility", "build-binary"]),
    ));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("attestations", "write"),
            ("contents", "read"),
            ("id-token", "write"),
        ]),
    ));
    let attest = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Attest exact stable release subjects"),
        ),
        ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("create-storage-record".to_owned(), Yaml::Bool(false)),
                (
                    "subject-path".to_owned(),
                    Yaml::str(format!(
                        "${{{{ runner.temp }}}}/velnor/preview-signed/{asset}\n${{{{ runner.temp }}}}/velnor/preview-signed/release.json"
                    )),
                ),
            ]),
        ),
    ]);
    let mut steps = vec![
        mise_step(&spec.linux_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        non_internal_step_to_yaml(
            "attest-binary",
            &spec.linux_acquire,
            &BTreeMap::default(),
            "ubuntu-26.04",
        )?,
        io::transfer_artifact(true, "producer", "preview-input"),
        io::operation(spec, "sign-package-release-v1", true, false)?,
        attest,
    ];
    steps.push(io::transfer_artifact(false, "signed", "preview-signed"));
    Ok(finish("attest-binary", fields, steps))
}

fn publish_job(spec: &ConsumerBinaryReleaseSpec) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Publish immutable stable release",
        Yaml::str("ubuntu-26.04"),
        75,
    );
    fields.push((
        "needs".to_owned(),
        needs(&["release-eligibility", "build-binary", "attest-binary"]),
    ));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("attestations", "read"),
            ("contents", "write"),
        ]),
    ));
    fields.push((
        "environment".to_owned(),
        Yaml::Map(vec![(
            "name".to_owned(),
            Yaml::str("consumer-binary-release"),
        )]),
    ));
    let steps = vec![
        io::transfer_artifact(true, "signed", "preview-publish-input"),
        mise_step(&spec.linux_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        non_internal_step_to_yaml(
            "publish-binary",
            &spec.linux_acquire,
            &BTreeMap::default(),
            "ubuntu-26.04",
        )?,
        io::operation(spec, "publish-package-release-v1", true, true)?,
    ];
    Ok(finish("publish-binary", fields, steps))
}
