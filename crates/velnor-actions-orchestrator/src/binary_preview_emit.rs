//! Independently enabled native preview generation, composed from source metadata.

use crate::{
    OrchestratorError,
    internal::internal,
    pins::{
        consumer_acquire_step, resolve_mise_setup_for_release_target,
        resolve_verification_mise_setup,
    },
    prepare::GenerationPreparation,
    release_identity::origin_repository,
    utf8::strings_of,
};
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, ReleaseTarget,
    package_release::PackageReleaseChannel,
};
use velnor_actions_mise::{MiseInstall, PinnedTool, PinnedToolExec};
use velnor_actions_rust::resolve_preview_products;
use velnor_actions_workflow_renderer::{
    RenderedFile,
    schema2::{ConsumerPreviewSpec, render_consumer_preview},
};

pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(release) = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release)
    else {
        return Ok(Vec::new());
    };
    let preview = &release.preview;
    if !preview.enabled {
        return Ok(Vec::new());
    }
    preview.validate(".velnor/config.toml")?;
    crate::binary_release_emit::require_tracked_lockfile(prep, release)?;
    let plan = release_plan(prep, release)?;
    let version = env!("CARGO_PKG_VERSION");
    let linux_acquire = consumer_acquire_step("ubuntu-26.04", version, &prep.discovery)?;
    let producer_acquire =
        consumer_acquire_step(preview.producer.runner.runs_on(), version, &prep.discovery)?;
    let install =
        MiseInstall::new(vec![PinnedTool::Gh]).map_err(|error| internal(&error.to_string()))?;
    let rust_install =
        MiseInstall::new(vec![PinnedTool::Rust]).map_err(|error| internal(&error.to_string()))?;
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        Vec::<OsString>::new(),
    )
    .map_err(|error| internal(&error.to_string()))?;
    let spec = ConsumerPreviewSpec {
        generator_version: version.to_owned(),
        plan,
        producer_runner: preview.producer.runner,
        producer_timeout_minutes: preview.producer.timeout_minutes,
        linux_acquire,
        producer_acquire,
        producer_setup: resolve_verification_mise_setup(&prep.config, preview.producer.runner)?,
        linux_setup: resolve_mise_setup_for_release_target(
            &prep.config,
            ReleaseTarget::LinuxX86_64,
        )?,
        install_rust_argv: strings_of(rust_install.argv(&prep.catalog))
            .map_err(|error| internal(&error))?,
        install_gh_argv: strings_of(install.argv(&prep.catalog))
            .map_err(|error| internal(&error))?,
        gh_argv: strings_of(exec.argv(&prep.catalog)).map_err(|error| internal(&error))?,
    };
    Ok(vec![render_consumer_preview(&spec)?])
}

pub(crate) fn release_plan(
    prep: &GenerationPreparation,
    release: &velnor_actions_contract::config::RustBinaryReleaseConfig,
) -> Result<PackageReleasePlan, OrchestratorError> {
    let preview = &release.preview;
    let workspace_root = Path::new(&release.manifest_path)
        .parent()
        .and_then(Path::to_str)
        .filter(|path| *path != ".")
        .unwrap_or("");
    let mut workspaces = prep
        .discovery
        .workspaces
        .iter()
        .filter(|workspace| workspace.record.workspace_root == workspace_root);
    let workspace = workspaces
        .next()
        .ok_or_else(|| internal("preview_workspace_missing"))?;
    if workspaces.next().is_some() {
        return Err(internal("preview_workspace_ambiguous"));
    }
    let resolved = resolve_preview_products(preview, &workspace.record)?;
    let mut products = preview.products.clone();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    for product in &mut products {
        product.bins.sort();
        product.targets.sort();
    }
    let mut companions = preview.companions.clone();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: origin_repository(&prep.root)?,
        default_branch: prep.default_branch.clone(),
        base_version: resolved.base_version,
        products,
        companions,
    })
}
