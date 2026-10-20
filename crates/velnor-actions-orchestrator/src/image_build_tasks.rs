//! Resolve `[[images]]` declarations against the checked-out repository.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path};

use velnor_actions_contract::config::analyze_dockerfile;
use velnor_actions_contract::{ImageConfig, ImagePrebuilt, Step, VelnorConfig};
use velnor_actions_mise::{GitRequest, ToolCatalog};
use velnor_actions_workflow_renderer::verification_jobs::ImageBuildPolicy;

use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::discover::Discovery;
use crate::safe_read::{RepoRead, read_repo_file};

const MAX_DOCKERFILE_BYTES: u64 = 1024 * 1024;

/// One sorted policy per declared image.
pub(crate) fn policies(
    root: &Path,
    config: &VelnorConfig,
    prebuilt: &PrebuiltInputs<'_>,
) -> Result<Vec<ImageBuildPolicy>, OrchestratorError> {
    config
        .images
        .iter()
        .map(|image| resolve(root, image, prebuilt))
        .collect()
}

/// Planning inputs the prebuilt (`cargo_bin`) steps need.
pub(crate) struct PrebuiltInputs<'a> {
    pub(crate) discovery: &'a Discovery,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) fetch_roots: &'a [String],
    /// Helper staging step (consumer policy); the private-source block needs it.
    pub(crate) acquire: Option<&'a Step>,
    /// Private-source wiring; `None` without `[private_sources]`.
    pub(crate) private: Option<&'a PrivateWiring>,
}

fn resolve(
    root: &Path,
    image: &ImageConfig,
    prebuilt: &PrebuiltInputs<'_>,
) -> Result<ImageBuildPolicy, OrchestratorError> {
    let mut dockerfile_sha256 = BTreeMap::new();
    for platform in &image.platforms {
        let path = image
            .dockerfile
            .for_platform(*platform)
            .ok_or_else(|| failure(&image.id, "image_dockerfile_platform_missing"))?;
        require_plain_path(root, path, true, &image.id)?;
        require_tracked(root, path, &image.id)?;
        let text = match read_repo_file(root, path, MAX_DOCKERFILE_BYTES)? {
            RepoRead::Text(text) => text,
            RepoRead::Absent => return Err(failure(&image.id, "image_dockerfile_missing")),
        };
        let facts =
            analyze_dockerfile(&text).map_err(|problem| failure(&image.id, &problem.code()))?;
        require_stage_copied(image, &facts.context_sources)?;
        dockerfile_sha256.insert(*platform, crate::generator::sha256_hex(text.as_bytes()));
    }
    if image.context != "." {
        require_plain_path(root, &image.context, false, &image.id)?;
    }
    if let Some(ImagePrebuilt::CargoBin { stage, .. }) = &image.prebuilt
        && fs::symlink_metadata(root.join(stage)).is_ok()
    {
        return Err(failure(&image.id, "image_prebuilt_stage_exists"));
    }
    let prebuilt_steps = crate::image_prebuilt::steps(image, prebuilt)?;
    Ok(ImageBuildPolicy {
        image: image.clone(),
        dockerfile_sha256,
        prebuilt_steps,
    })
}

/// A prebuilt artifact must not pre-exist and the Dockerfile must copy it.
fn require_stage_copied(
    image: &ImageConfig,
    sources: &std::collections::BTreeSet<String>,
) -> Result<(), OrchestratorError> {
    let Some(ImagePrebuilt::CargoBin { stage, .. }) = &image.prebuilt else {
        return Ok(());
    };
    let relative = if image.context == "." {
        stage.as_str()
    } else {
        stage
            .strip_prefix(&format!("{}/", image.context))
            .unwrap_or(stage)
    };
    let copied = sources.iter().any(|source| {
        let source = source.trim_end_matches('/');
        source == relative
            || source == "."
            || source.is_empty()
            || relative.starts_with(&format!("{source}/"))
    });
    if copied {
        Ok(())
    } else {
        Err(failure(&image.id, "image_prebuilt_stage_not_copied"))
    }
}

/// Every component exists, is not a symlink, and the leaf has the right type.
fn require_plain_path(
    root: &Path,
    path: &str,
    leaf_is_file: bool,
    image: &str,
) -> Result<(), OrchestratorError> {
    let components = Path::new(path).components().collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(failure(image, "image_path"));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| failure(image, "image_path_missing_or_unreadable"))?;
        if metadata.file_type().is_symlink() {
            return Err(failure(image, "image_path_symlink"));
        }
        let leaf = index + 1 == components.len();
        let ok = if leaf && leaf_is_file {
            metadata.is_file()
        } else {
            metadata.is_dir()
        };
        if !ok {
            return Err(failure(image, "image_path_wrong_type"));
        }
    }
    Ok(())
}

fn require_tracked(root: &Path, path: &str, image: &str) -> Result<(), OrchestratorError> {
    let output = GitRequest::ls_files(vec![
        OsString::from("--error-unmatch"),
        OsString::from("--"),
        OsString::from(path),
    ])
    .run_in(root)
    .map_err(|_| failure(image, "image_dockerfile_tracking_check"))?;
    let listed = output
        .stdout_text("git")
        .map_err(|_| failure(image, "image_dockerfile_tracking_check"))?;
    if !output.success || listed.trim_end_matches('\n') != path {
        return Err(failure(image, "image_dockerfile_must_be_tracked"));
    }
    Ok(())
}

fn failure(image: &str, problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("images.{image}:{problem}"),
    }
}
