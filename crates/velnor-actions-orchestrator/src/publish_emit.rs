// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::config::{
    PublishBundle, RELEASE_TAG_GLOB, RustReleaseConfig, VersionSource,
};
use velnor_actions_contract::{ImageConfig, WorkflowPolicy};
use velnor_actions_rust::git_pins::unpinned_git_sources;
use velnor_actions_workflow_renderer::publish_render::render_publish_workflow;
use velnor_actions_workflow_renderer::publish_spec::{PublishTriggers, PublishWorkflowSpec};
use velnor_actions_workflow_renderer::release_config::{
    ReleasePlzConfig, ReleasePlzPackage, render_git_tag_release_plz_config,
};
use velnor_actions_workflow_renderer::release_tree::{RELEASE_CONFIG_PATH, ReleaseRenderContext};
use velnor_actions_workflow_renderer::{MiseSetup, render::RenderedFile};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::config::private_wiring::PrivateWiring;
use crate::prepare::GenerationPreparation;
use crate::publish_steps::{BundlePlan, Inputs, PromotionPlan, VersionCheck, assemble_jobs};
use crate::release_identity::origin_repository;

#[path = "publish_maven_emit.rs"]
mod maven_emit;
#[path = "publish_npm_emit.rs"]
mod npm_emit;

const PUBLISH_WORKFLOW_NAME: &str = "Velnor Publish";
const DEFAULT_ENVIRONMENT: &str = "release";

pub(crate) fn publish_files(
    prep: &GenerationPreparation,
    mise: &MiseSetup,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let bundles = &prep.config.publish.bundles;
    let npm = &prep.config.publish.npm;
    let maven = &prep.config.publish.maven;
    let images = &prep.config.images;
    let git_tag = crate::release_emit::git_tag_release(prep)?;
    if bundles.is_empty()
        && npm.is_empty()
        && maven.is_empty()
        && git_tag.is_none()
        && images.is_empty()
    {
        return Ok(Vec::new());
    }
    require_consumer_policy(prep)?;
    let plans = bundles
        .iter()
        .enumerate()
        .map(|(index, bundle)| resolve_bundle(&prep.root, index, bundle))
        .collect::<Result<Vec<_>, _>>()?;
    check_tag_collisions(prep)?;
    let promotions = resolve_promotions(&prep.root, images)?;
    let repository = origin_repository(&prep.root)?;
    let tag_config = git_tag
        .map(|release| git_tag_config(prep, release, &repository))
        .transpose()?;
    let catalog = prep.emission_catalog();
    let npm_plans = npm
        .iter()
        .enumerate()
        .map(|(index, entry)| npm_emit::resolve(prep, index, entry, &catalog))
        .collect::<Result<Vec<_>, _>>()?;
    let private = PrivateWiring::load(&prep.root, &prep.config, &catalog)?;
    let maven_plans = maven
        .iter()
        .enumerate()
        .map(|(index, entry)| maven_emit::resolve(prep, private.as_ref(), index, entry))
        .collect::<Result<Vec<_>, _>>()?;
    let jobs = assemble_jobs(&Inputs {
        repository: &repository,
        branch: &prep.default_branch,
        label: &prep.runner_label,
        environment: DEFAULT_ENVIRONMENT,
        provenance: prep.config.publish.provenance,
        mise,
        catalog: &catalog,
        bundles: &plans,
        npm: &npm_plans,
        maven: &maven_plans,
        git_tag_manifest: git_tag.map(|release| release.manifest_path.as_str()),
        images,
        promotions: &promotions,
    })?;
    let spec = PublishWorkflowSpec {
        name: PUBLISH_WORKFLOW_NAME.to_owned(),
        repository,
        triggers: PublishTriggers {
            branches: if git_tag.is_some() || !images.is_empty() {
                vec![prep.default_branch.clone()]
            } else {
                Vec::new()
            },
            tag_glob: (!plans.is_empty()
                || !npm_plans.is_empty()
                || !maven_plans.is_empty()
                || !promotions.is_empty())
            .then(|| RELEASE_TAG_GLOB.to_owned()),
        },
        jobs,
        staged_binary: (!maven_plans.is_empty())
            .then(|| prep.workflow.context.staged_binary.clone()),
    };
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: prep.runner_label.clone(),
    };
    let mut files = vec![render_publish_workflow(&spec, &ctx)?];
    files.extend(tag_config);
    Ok(files)
}

fn require_consumer_policy(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    let publish = &prep.config.publish;
    let key = if !publish.bundles.is_empty() {
        "publish.bundles"
    } else if !publish.npm.is_empty() {
        "publish.npm"
    } else if !publish.maven.is_empty() {
        "publish.maven"
    } else if !prep.config.images.is_empty() {
        "images"
    } else {
        return Ok(());
    };
    if prep.config.workflow.policy == WorkflowPolicy::ConsumerV1 {
        return Ok(());
    }
    Err(OrchestratorError::config(
        CONFIG_REL,
        key,
        "publish_requires_consumer_policy",
    ))
}

fn git_tag_config(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
    repository: &str,
) -> Result<RenderedFile, OrchestratorError> {
    let json =
        crate::release_emit::release_metadata_json(prep, release.manifest_path.as_str())?;
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
        (None, Some(path), Some(source)) => manifest_check(root, &key, path, source)?,
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

fn manifest_check(
    root: &Path,
    key: &str,
    path: &str,
    source: VersionSource,
) -> Result<VersionCheck, OrchestratorError> {
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
    Ok(VersionCheck::Manifest(source, path.to_owned()))
}

fn resolve_promotions<'a>(
    root: &Path,
    images: &'a [ImageConfig],
) -> Result<Vec<PromotionPlan<'a>>, OrchestratorError> {
    let mut plans = Vec::new();
    for (index, image) in images.iter().enumerate() {
        let Some(path) = &image.version_file else {
            continue;
        };
        let key = format!("images[{index}]");
        let source = VersionSource::for_path(path).ok_or_else(|| {
            OrchestratorError::config(
                CONFIG_REL,
                format!("{key}.version_file"),
                format!("unsupported_image_version_file:{path}"),
            )
        })?;
        plans.push(PromotionPlan {
            image,
            check: manifest_check(root, &key, path, source)?,
        });
    }
    Ok(plans)
}

fn check_tag_collisions(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    let release: Option<&RustReleaseConfig> = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|stack| &stack.release)
        .filter(|release| release.enabled);
    let packages: std::collections::BTreeSet<&str> = release
        .map(|release| release.packages.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let mut seen = std::collections::BTreeSet::new();
    let bundles = prep
        .config
        .publish
        .bundles
        .iter()
        .map(|b| ("publish.bundles", &b.id));
    let npm = prep
        .config
        .publish
        .npm
        .iter()
        .map(|e| ("publish.npm", &e.id));
    let maven = prep
        .config
        .publish
        .maven
        .iter()
        .map(|e| ("publish.maven", &e.id));
    let images = prep
        .config
        .images
        .iter()
        .filter(|image| image.version_file.is_some())
        .map(|image| ("images", &image.id));
    for (key, id) in bundles.chain(npm).chain(maven).chain(images) {
        if packages.contains(id.as_str()) {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                key,
                format!("artifact_id_collides_with_package:{id}"),
            ));
        }
        if !seen.insert(id.as_str()) {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                key,
                format!("artifact_id_collides:{id}"),
            ));
        }
    }
    Ok(())
}
