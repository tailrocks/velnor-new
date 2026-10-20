//! Publication emission: `[[publish.bundles]]` to `publish.yml`.
//!
//! The file exists only under `consumer-v1` with at least one declared
//! bundle. Generation reads each bundle's version manifest and checks
//! its paths against the work tree, so a stale declaration fails at
//! generation time; the generated authority job repeats the version
//! check against the tagged commit at run time.

use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{PublishBundle, RELEASE_TAG_GLOB, RustReleaseConfig};
use velnor_actions_rust::git_pins::unpinned_git_sources;
use velnor_actions_workflow_renderer::publish_render::render_publish_workflow_with_routing;
use velnor_actions_workflow_renderer::publish_spec::{PublishTriggers, PublishWorkflowSpec};
use velnor_actions_workflow_renderer::release_config::{
    ReleasePlzConfig, ReleasePlzPackage, render_git_tag_release_plz_config,
};
use velnor_actions_workflow_renderer::release_tree::{RELEASE_CONFIG_PATH, ReleaseRenderContext};
use velnor_actions_workflow_renderer::{MiseSetup, WorkflowRoutingPolicy, render::RenderedFile};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::prepare::GenerationPreparation;
use crate::publish_steps::{BundlePlan, Inputs, VersionCheck, assemble_jobs};
use crate::release_identity::origin_repository;

#[path = "publish_job.rs"]
pub(crate) mod baseline;
#[path = "publish_release_steps.rs"]
pub(crate) mod release;

#[cfg(test)]
#[path = "publish_scripts_tests.rs"]
mod script_tests;

/// Publication workflow display name.
const PUBLISH_WORKFLOW_NAME: &str = "Velnor Publish";
/// Protected environment guarding asset upload when none is configured.
const DEFAULT_ENVIRONMENT: &str = "release";

/// Render `publish.yml` (and the `git-tag` release-plz config), or nothing.
///
/// # Errors
///
/// Returns config errors for a non-consumer policy, unreadable or
/// non-literal version manifests, missing or symlinked bundle paths and
/// private git sources not pinned to a tag; identity errors for an
/// unresolvable origin; render errors for invalid assembled specs.
pub(crate) fn publish_files(
    prep: &GenerationPreparation,
    mise: &MiseSetup,
    routing: &WorkflowRoutingPolicy,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let bundles = &prep.config.publish.bundles;
    let git_tag = crate::release_emit::git_tag_release(prep)?;
    if bundles.is_empty() && git_tag.is_none() {
        return Ok(Vec::new());
    }
    if !bundles.is_empty() && prep.config.workflow.policy != WorkflowPolicy::ConsumerV1 {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "publish.bundles",
            "publish_requires_consumer_policy",
        ));
    }
    let plans = bundles
        .iter()
        .enumerate()
        .map(|(index, bundle)| resolve_bundle(&prep.root, index, bundle))
        .collect::<Result<Vec<_>, _>>()?;
    check_tag_collisions(prep)?;
    let repository = origin_repository(&prep.root)?;
    let tag_config = git_tag
        .map(|release| git_tag_config(prep, release, &repository))
        .transpose()?;
    let catalog = prep.emission_catalog();
    let jobs = assemble_jobs(&Inputs {
        repository: &repository,
        branch: &prep.default_branch,
        label: &prep.runner_label,
        environment: DEFAULT_ENVIRONMENT,
        mise,
        catalog: &catalog,
        bundles: &plans,
        git_tag_manifest: git_tag.map(|release| release.manifest_path.as_str()),
    })?;
    let spec = PublishWorkflowSpec {
        name: PUBLISH_WORKFLOW_NAME.to_owned(),
        repository,
        triggers: PublishTriggers {
            branches: if git_tag.is_some() {
                vec![prep.default_branch.clone()]
            } else {
                Vec::new()
            },
            tag_glob: (!plans.is_empty()).then(|| RELEASE_TAG_GLOB.to_owned()),
        },
        jobs,
    };
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: prep.runner_label.clone(),
    };
    let mut files = vec![render_publish_workflow_with_routing(&spec, &ctx, routing)?];
    files.extend(tag_config);
    Ok(files)
}

/// Select the `git-tag` packages and render their release-plz config.
///
/// Also enforces the pin rule: a git source on the repository owner's own
/// GitHub organization must be pinned to a `tag`.
fn git_tag_config(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
    repository: &str,
) -> Result<RenderedFile, OrchestratorError> {
    let json = crate::release_emit::release_metadata_json(
        &prep.root,
        release.manifest_path.as_str(),
        &prep.catalog,
    )?;
    let owner = repository.split('/').next().unwrap_or_default();
    let unpinned =
        unpinned_git_sources(&json, owner).map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    if let Some(first) = unpinned.first() {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release",
            format!(
                "private_git_dependency_unpinned:{}:{}",
                first.package, first.source
            ),
        ));
    }
    let selection = crate::release_emit::select_from_json(prep, release, &json)?;
    if selection.is_empty() {
        return Err(OrchestratorError::Contract {
            problem: "release_nothing_selected".to_owned(),
        });
    }
    let config = ReleasePlzConfig {
        tag_pattern: release.tag_name.clone(),
        semver_check: false,
        packages: selection
            .packages
            .iter()
            .map(|package| ReleasePlzPackage {
                name: package.name.clone(),
                publish_features: Vec::new(),
            })
            .collect(),
    };
    let bytes = render_git_tag_release_plz_config(&config, env!("CARGO_PKG_VERSION"))?;
    Ok(RenderedFile {
        path: RELEASE_CONFIG_PATH.to_owned(),
        bytes,
    })
}

/// Resolve one bundle: check its paths and read its version source.
fn resolve_bundle<'a>(
    root: &Path,
    index: usize,
    bundle: &'a PublishBundle,
) -> Result<BundlePlan<'a>, OrchestratorError> {
    let key = format!("publish.bundles[{index}]");
    for path in &bundle.paths {
        let meta = std::fs::symlink_metadata(root.join(path)).map_err(|_| {
            OrchestratorError::config(
                CONFIG_REL,
                format!("{key}.paths"),
                format!("missing_path:{path}"),
            )
        })?;
        if meta.is_symlink() {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                format!("{key}.paths"),
                format!("symlink_path:{path}"),
            ));
        }
    }
    let check = match (
        &bundle.version,
        &bundle.version_file,
        bundle.version_source(),
    ) {
        (Some(version), _, _) => VersionCheck::Inline(version.clone()),
        (None, Some(path), Some(source)) => {
            let text = std::fs::read_to_string(root.join(path)).map_err(|_| {
                OrchestratorError::config(
                    CONFIG_REL,
                    format!("{key}.version_file"),
                    format!("unreadable_version_file:{path}"),
                )
            })?;
            if source.read(&text).is_none() {
                return Err(OrchestratorError::config(
                    CONFIG_REL,
                    format!("{key}.version_file"),
                    format!("version_not_literal:{path}"),
                ));
            }
            VersionCheck::Manifest(source, path.clone())
        }
        _ => {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                format!("{key}.version_file"),
                "missing_version_source",
            ));
        }
    };
    Ok(BundlePlan { bundle, check })
}

/// Reject a bundle id that is also a released Rust package name.
///
/// Both kinds mint tags in the same `<id>-v<X.Y.Z>` namespace, so a shared
/// name would let one tag mean two artifacts.
fn check_tag_collisions(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    let release: Option<&RustReleaseConfig> =
        prep.config.stacks.rust.as_ref().map(|stack| &stack.release);
    let Some(release) = release.filter(|release| release.enabled) else {
        return Ok(());
    };
    for bundle in &prep.config.publish.bundles {
        if release.packages.contains(&bundle.id) {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                "publish.bundles",
                format!("artifact_id_collides_with_package:{}", bundle.id),
            ));
        }
    }
    Ok(())
}
