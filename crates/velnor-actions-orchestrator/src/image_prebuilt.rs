//! Prebuilt `cargo_bin` steps for an image build job.
//!
//! The binary is compiled by Velnor outside `BuildKit`, from pinned tools and
//! sources prepared without credentials, then staged into the build context.
//! The Dockerfile then only copies it into a runtime image, so the build never
//! needs a secret and the compile is not duplicated inside the image build.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

use velnor_actions_contract::{ImageConfig, ImagePlatform, ImagePrebuilt, Step};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_rust::PackageRecord;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::image_build_tasks::PrebuiltInputs;
use crate::utf8::strings_of;

const BUILD_STEP: &str = "Build release binary";
const STAGE_STEP: &str = "Stage release binary";

/// Steps shared by every platform job of one `cargo_bin` image.
///
/// Private Cargo sources are retrieved exactly like a crate job does: the
/// mint, fetch and revoke block runs after the helper is staged and before
/// the Cargo source fetch, so no credential outlives the Velnor-owned fetch.
pub(crate) fn steps(
    image: &ImageConfig,
    inputs: &PrebuiltInputs<'_>,
) -> Result<Vec<Step>, OrchestratorError> {
    let PrebuiltInputs {
        discovery,
        catalog,
        fetch_roots,
        acquire,
        private,
    } = *inputs;
    let Some(ImagePrebuilt::CargoBin {
        package,
        bin,
        stage,
    }) = &image.prebuilt
    else {
        return Ok(Vec::new());
    };
    if image.platforms.contains(&ImagePlatform::LinuxArm64) {
        // The shared Mise tools cache is qualified for the Ubuntu x86-64 hosted
        // images only (runtime identity, composite prelude, Mise pins). Arm64
        // needs that cache contract extended first; fail rather than skip it.
        return Err(failure(
            &image.id,
            "image_prebuilt_platform_unsupported:linux_arm64",
        ));
    }
    let record = locate(discovery, package, bin, &image.id)?;
    let roots = crate::source_prep::crate_fetch_roots(&record.manifest, fetch_roots, false);
    if roots.is_empty() {
        return Err(failure(&image.id, "image_prebuilt_workspace_unresolved"));
    }
    let mut env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    for (native, mise) in [
        ("CARGO_HOME", "MISE_CARGO_HOME"),
        ("RUSTUP_HOME", "MISE_RUSTUP_HOME"),
    ] {
        env.insert(native.to_owned(), env[mise].clone());
    }
    let source_steps = crate::source_prep::fetch_steps_for_crate(catalog, &roots, false)?;
    let mut steps = vec![crate::matrix_step::prepare_crate_tools_step(
        catalog, true, false, false, false,
    )?];
    steps.extend(crate::source_cache::restore_step_for_crate(&roots, true)?);
    let block = match private {
        Some(private) => private.steps_for_roots(&roots)?,
        None => Vec::new(),
    };
    if !block.is_empty() {
        steps.extend(acquire.cloned());
        steps.extend(block);
    }
    steps.extend(source_steps);
    let target_dir = format!("${{{{ runner.temp }}}}/velnor/image-target/{}", image.id);
    let mut build_env = env;
    build_env.insert("CARGO_TARGET_DIR".to_owned(), target_dir);
    steps.push(shell(
        BUILD_STEP,
        build_argv(&record.manifest, package, bin, catalog)?,
        &build_env,
    )?);
    steps.push(shell(
        STAGE_STEP,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            stage_script(&image.id, bin, stage),
        ],
        &build_env,
    )?);
    Ok(steps)
}

fn shell(
    name: &str,
    argv: Vec<String>,
    env: &BTreeMap<String, String>,
) -> Result<Step, OrchestratorError> {
    Ok(velnor_actions_workflow_renderer::shell_step(
        name,
        argv,
        env.clone(),
    )?)
}

/// The one in-workspace package that owns `bin`.
fn locate<'a>(
    discovery: &'a Discovery,
    package: &str,
    bin: &str,
    image: &str,
) -> Result<&'a PackageRecord, OrchestratorError> {
    let mut found = discovery.workspaces.iter().flat_map(|planned| {
        planned
            .record
            .packages
            .iter()
            .filter(|p| p.in_workspace && !p.external && p.name == package)
    });
    let record = found
        .next()
        .ok_or_else(|| failure(image, "image_prebuilt_package_not_discovered"))?;
    if found.next().is_some() {
        return Err(failure(image, "image_prebuilt_package_ambiguous"));
    }
    let mut targets = record
        .targets
        .iter()
        .filter(|t| t.kind == "bin" && t.name == bin);
    let target = targets
        .next()
        .ok_or_else(|| failure(image, "image_prebuilt_bin_not_discovered"))?;
    if targets.next().is_some() || !target.required_features.is_empty() {
        return Err(failure(image, "image_prebuilt_bin_unsupported"));
    }
    Ok(record)
}

fn build_argv(
    manifest: &str,
    package: &str,
    bin: &str,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let args = [
        "build",
        "--release",
        "--locked",
        "--offline",
        "--manifest-path",
        manifest,
        "--package",
        package,
        "--bin",
        bin,
    ];
    let request = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        args.iter().map(OsString::from).collect(),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(request.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Copy the binary into the context; refuse to overwrite anything present.
fn stage_script(id: &str, bin: &str, stage: &str) -> String {
    let dir = stage.rsplit_once('/').map_or(".", |(dir, _)| dir);
    format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; test ! -e '{stage}'; mkdir -p '{dir}'; cp -- \"$RUNNER_TEMP/velnor/image-target/{id}/release/{bin}\" '{stage}'"
    )
}

fn failure(image: &str, problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("images.{image}:{problem}"),
    }
}

#[cfg(test)]
#[path = "image_prebuilt_tests.rs"]
mod tests;
