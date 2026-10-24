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
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::Path,
};
use velnor_actions_contract::{
    PackageReleasePlanProduct, ReleaseTarget, package_release::PackageReleaseChannel,
    package_release::PackageReleasePayloadFormat,
};
use velnor_actions_mise::{MiseInstall, PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_rust::resolve_release_products;
use velnor_actions_workflow_renderer::{
    RenderedFile,
    schema2::{
        ConsumerReleaseSpec, ConsumerReleaseTapSpec, render_consumer_preview,
        render_release_tap_reconcile,
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
    if !release.preview.enabled {
        return Ok(Vec::new());
    }
    let spec = specification(prep, release, PackageReleaseChannel::Preview)?;
    let mut files = vec![render_consumer_preview(&spec)?];
    if spec.homebrew_tap.is_some() {
        files.push(render_release_tap_reconcile(&spec)?);
    }
    Ok(files)
}

pub(crate) fn specification(
    prep: &GenerationPreparation,
    release: &velnor_actions_contract::config::RustBinaryReleaseConfig,
    channel: PackageReleaseChannel,
) -> Result<ConsumerReleaseSpec, OrchestratorError> {
    if prep.default_branch != "main" {
        return Err(internal("preview_default_branch_must_be_main"));
    }
    release.validate(".velnor/config.toml")?;
    crate::binary_release_emit::require_tracked_lockfile(prep, release)?;
    let policy = release_policy(prep, release, channel)?;
    let version = env!("CARGO_PKG_VERSION");
    let linux_acquire = consumer_acquire_step("ubuntu-26.04", version, &prep.discovery)?;
    let producer_acquire =
        consumer_acquire_step(release.producer.runner.runs_on(), version, &prep.discovery)?;
    let mut producer_acquire_by_target = BTreeMap::new();
    let mut producer_setup_by_target = BTreeMap::new();
    if channel == PackageReleaseChannel::Preview {
        for target in ReleaseTarget::ALL {
            producer_acquire_by_target.insert(
                target,
                crate::pins::consumer_acquire_for_target(
                    target,
                    version,
                    prep.discovery.consumer_manifest_json.as_deref(),
                )?,
            );
            producer_setup_by_target.insert(
                target,
                resolve_mise_setup_for_release_target(&prep.config, target)?,
            );
        }
    }
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
    let spec = ConsumerReleaseSpec {
        checkout_lfs: prep.config.workflow.checkout_lfs,
        generator_version: version.to_owned(),
        policy,
        producer_runner: release.producer.runner,
        producer_timeout_minutes: release.producer.timeout_minutes,
        publication_timeout_minutes: release.publication_timeout_minutes,
        linux_acquire,
        producer_acquire,
        producer_acquire_by_target,
        producer_setup_by_target,
        producer_setup: resolve_verification_mise_setup(&prep.config, release.producer.runner)?,
        linux_setup: resolve_mise_setup_for_release_target(
            &prep.config,
            ReleaseTarget::LinuxX86_64,
        )?,
        install_rust_argv: strings_of(rust_install.argv(&emission))
            .map_err(|error| internal(&error))?,
        verify_rust_argv: crate::workflow::verify_rust_argv(&emission),
        producer_source_prep,
        producer_cargo_env,
        install_gh_argv: strings_of(install.argv(&native_catalog))
            .map_err(|error| internal(&error))?,
        gh_argv: strings_of(exec.argv(&native_catalog)).map_err(|error| internal(&error))?,
        homebrew_tap: tap_spec(prep)?.filter(|tap| tap.policy.destination(channel).is_some()),
        stable: None,
    };
    Ok(spec)
}

fn tap_spec(
    prep: &GenerationPreparation,
) -> Result<Option<ConsumerReleaseTapSpec>, OrchestratorError> {
    let Some(policy) = prep
        .config
        .distribution
        .homebrew_tap
        .as_ref()
        .filter(|tap| tap.enabled)
    else {
        return Ok(None);
    };
    let native_runner = velnor_actions_contract::config::CheckRunner {
        label: "ubuntu-26.04".to_owned(),
        platform: velnor_actions_contract::config::CheckPlatform::LinuxX64,
        executor: velnor_actions_contract::config::CheckExecutor::Hosted,
        container: None,
    };
    let runner = policy
        .updater
        .as_ref()
        .map_or(&native_runner, |updater| &updater.runner);
    let formula_spec = prep.config.distribution.homebrew_formula.as_ref();
    if policy.updater.is_none() != formula_spec.is_some() {
        return Err(internal("homebrew_tap_formula_route_mismatch"));
    }
    Ok(Some(ConsumerReleaseTapSpec {
        policy: policy.clone(),
        formula_spec: formula_spec.cloned(),
        preparation_acquire: consumer_acquire_for_runner(
            runner,
            env!("CARGO_PKG_VERSION"),
            prep.discovery.consumer_manifest_json.as_deref(),
        )?,
        preparation_setup: resolve_mise_setup_for_release_target(
            &prep.config,
            runner.platform.release_target(),
        )?,
    }))
}

pub(crate) fn release_policy(
    prep: &GenerationPreparation,
    release: &velnor_actions_contract::config::RustBinaryReleaseConfig,
    channel: PackageReleaseChannel,
) -> Result<velnor_actions_contract::PackageReleasePolicy, OrchestratorError> {
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
    resolve_release_products(release, &workspace.record)?;
    let version = velnor_actions_rust::release_version_policy(release, &workspace.record)?;
    let mut products = release
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
    let mut companions = release.companions.clone();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    let policy = velnor_actions_contract::PackageReleasePolicy {
        schema: velnor_actions_contract::PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel,
        repository: origin_repository(&prep.root)?,
        default_branch: prep.default_branch.clone(),
        version,
        products,
        companions,
    };
    policy
        .validate()
        .map_err(|error| internal(&error.to_string()))?;
    Ok(policy)
}
