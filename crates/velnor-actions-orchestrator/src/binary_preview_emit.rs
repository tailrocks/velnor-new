//! Independently enabled native preview generation, composed from source metadata.

use crate::{
    OrchestratorError,
    internal::internal,
    pins::{
        consumer_acquire_for_runner, consumer_acquire_step, resolve_mise_setup_for_release_target,
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
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct, ReleaseTarget,
    package_release::PackageReleaseChannel, package_release::PackageReleasePayloadFormat,
};
use velnor_actions_mise::{MiseInstall, PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_rust::resolve_preview_products;
use velnor_actions_workflow_renderer::{
    RenderedFile,
    schema2::{
        ConsumerPreviewSpec, ConsumerPreviewTapSpec, render_consumer_preview,
        render_preview_tap_reconcile,
    },
};

#[path = "preview_source_prep.rs"]
mod source_preparation;

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
    if prep.default_branch != "main" {
        return Err(internal("preview_default_branch_must_be_main"));
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
    let native_catalog = ToolCatalog::pinned();
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        Vec::<OsString>::new(),
    )
    .map_err(|error| internal(&error.to_string()))?;
    let emission = prep.emission_catalog();
    let (producer_source_prep, producer_cargo_env) = source_preparation::for_roots(
        &emission,
        &crate::source_prep::lockful_roots(&prep.root, &prep.discovery.workspaces),
    )?;
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
        install_rust_argv: strings_of(rust_install.argv(&emission))
            .map_err(|error| internal(&error))?,
        producer_source_prep,
        producer_cargo_env,
        install_gh_argv: strings_of(install.argv(&native_catalog))
            .map_err(|error| internal(&error))?,
        gh_argv: strings_of(exec.argv(&native_catalog)).map_err(|error| internal(&error))?,
        homebrew_tap: tap_spec(prep)?,
    };
    let mut files = vec![render_consumer_preview(&spec)?];
    if spec.homebrew_tap.is_some() {
        files.push(render_preview_tap_reconcile(&spec)?);
    }
    Ok(files)
}

fn tap_spec(
    prep: &GenerationPreparation,
) -> Result<Option<ConsumerPreviewTapSpec>, OrchestratorError> {
    let Some(policy) = prep
        .config
        .distribution
        .homebrew_tap
        .as_ref()
        .filter(|tap| tap.enabled)
    else {
        return Ok(None);
    };
    let runner = &policy.updater.runner;
    Ok(Some(ConsumerPreviewTapSpec {
        policy: policy.clone(),
        updater_acquire: consumer_acquire_for_runner(
            runner,
            env!("CARGO_PKG_VERSION"),
            prep.discovery.consumer_manifest_json.as_deref(),
        )?,
        updater_setup: resolve_mise_setup_for_release_target(
            &prep.config,
            runner.platform.release_target(),
        )?,
    }))
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
    let mut products = preview
        .products
        .iter()
        .map(|product| {
            let mut bins = product.bins.clone();
            bins.sort();
            let mut targets = product.targets.clone();
            targets.sort();
            PackageReleasePlanProduct {
                id: product.id.clone(),
                package: product.package.clone(),
                bins,
                targets,
                format: PackageReleasePayloadFormat::TarGz,
            }
        })
        .collect::<Vec<_>>();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    let mut companions = preview.companions.clone();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    let plan = PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: origin_repository(&prep.root)?,
        default_branch: prep.default_branch.clone(),
        base_version: resolved.base_version,
        products,
        companions,
    };
    plan.validate()
        .map_err(|error| internal(&error.to_string()))?;
    Ok(plan)
}

pub(crate) fn stable_release_plan(
    prep: &GenerationPreparation,
    release: &velnor_actions_contract::config::RustBinaryReleaseConfig,
) -> Result<PackageReleasePlan, OrchestratorError> {
    let workspace_root = Path::new(&release.manifest_path)
        .parent()
        .and_then(Path::to_str)
        .filter(|path| *path != ".")
        .unwrap_or("");
    let workspace = prep
        .discovery
        .workspaces
        .iter()
        .find(|item| item.record.workspace_root == workspace_root)
        .ok_or_else(|| internal("binary_release_workspace_missing"))?;
    let packages = workspace
        .record
        .packages
        .iter()
        .filter(|package| {
            package.in_workspace && !package.external && package.name == release.package
        })
        .collect::<Vec<_>>();
    let package = packages
        .first()
        .copied()
        .filter(|_| packages.len() == 1)
        .ok_or_else(|| internal("binary_release_package_not_discovered"))?;
    if !velnor_actions_contract::config::is_exact_semver(&package.version) {
        return Err(internal("binary_release_package_version_invalid"));
    }
    let bins = package
        .targets
        .iter()
        .filter(|target| target.kind == "bin" && target.name == release.bin)
        .collect::<Vec<_>>();
    if bins.len() != 1 || !bins[0].required_features.is_empty() {
        return Err(internal("binary_release_bin_not_supported"));
    }
    let plan = PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Stable,
        repository: origin_repository(&prep.root)?,
        default_branch: prep.default_branch.clone(),
        base_version: package.version.clone(),
        products: vec![PackageReleasePlanProduct {
            id: release.package.clone(),
            package: release.package.clone(),
            bins: vec![release.bin.clone()],
            targets: vec![ReleaseTarget::MacosArm64.triple().to_owned()],
            format: PackageReleasePayloadFormat::RawBinary,
        }],
        companions: Vec::new(),
    };
    plan.validate()
        .map_err(|error| internal(&error.to_string()))?;
    Ok(plan)
}
