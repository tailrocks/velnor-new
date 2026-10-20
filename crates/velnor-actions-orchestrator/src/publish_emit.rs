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
use velnor_actions_workflow_renderer::publish_render::render_publish_workflow;
use velnor_actions_workflow_renderer::publish_spec::{PublishTriggers, PublishWorkflowSpec};
use velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext;
use velnor_actions_workflow_renderer::{MiseSetup, render::RenderedFile};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::prepare::GenerationPreparation;
use crate::publish_steps::{BundlePlan, Inputs, VersionCheck, assemble_jobs};
use crate::release_identity::origin_repository;

/// Publication workflow display name.
const PUBLISH_WORKFLOW_NAME: &str = "Velnor Publish";
/// Protected environment guarding asset upload when none is configured.
const DEFAULT_ENVIRONMENT: &str = "release";

/// Render `publish.yml`, or nothing when no bundle is declared.
///
/// # Errors
///
/// Returns config errors for a non-consumer policy, unreadable or
/// non-literal version manifests, and missing or symlinked bundle
/// paths; identity errors for an unresolvable origin; render errors
/// for invalid assembled specs.
pub(crate) fn publish_files(
    prep: &GenerationPreparation,
    mise: &MiseSetup,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let bundles = &prep.config.publish.bundles;
    if bundles.is_empty() {
        return Ok(Vec::new());
    }
    if prep.config.workflow.policy != WorkflowPolicy::ConsumerV1 {
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
    let catalog = prep.emission_catalog();
    let jobs = assemble_jobs(&Inputs {
        repository: &repository,
        branch: &prep.default_branch,
        label: &prep.runner_label,
        environment: DEFAULT_ENVIRONMENT,
        mise,
        catalog: &catalog,
        bundles: &plans,
    })?;
    let spec = PublishWorkflowSpec {
        name: PUBLISH_WORKFLOW_NAME.to_owned(),
        repository,
        triggers: PublishTriggers {
            branches: Vec::new(),
            tag_glob: Some(RELEASE_TAG_GLOB.to_owned()),
        },
        jobs,
    };
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: prep.runner_label.clone(),
    };
    Ok(vec![render_publish_workflow(&spec, &ctx)?])
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
