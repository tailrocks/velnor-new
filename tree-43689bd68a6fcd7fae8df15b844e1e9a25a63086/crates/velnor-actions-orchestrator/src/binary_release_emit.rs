//! Opt-in consumer Rust binary release workflow emission.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;

use velnor_actions_contract::config::RustBinaryReleaseConfig;
use velnor_actions_contract::{ReleaseTarget, WorkflowPolicy};
use velnor_actions_mise::{
    GitRequest, MiseInstall, PinnedTool, PinnedToolExec, PrepareRustTarget, ToolCatalog,
};
use velnor_actions_rust::PackageRecord;
use velnor_actions_workflow_renderer::RenderedFile;
use velnor_actions_workflow_renderer::schema2::{
    ConsumerBinaryReleaseSpec, render_consumer_binary_release,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::pins::resolve_mise_setup_for_consumer_binary_release;
use crate::prepare::GenerationPreparation;
use crate::release_identity::origin_repository;
use crate::utf8::strings_of;

/// Render the generated binary workflow or no file when the option is off.
pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(config) = enabled_config(prep)? else {
        return Ok(Vec::new());
    };
    let package = selected_package(prep, config)?;
    require_tracked_lockfile(prep, config)?;
    let repository = origin_repository(&prep.root)?;
    let catalog = ToolCatalog::pinned();
    let spec = ConsumerBinaryReleaseSpec {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        repository,
        default_branch: prep.default_branch.clone(),
        manifest_path: config.manifest_path.clone(),
        package: package.name.clone(),
        bin: config.bin.clone(),
        linux_setup: resolve_mise_setup_for_consumer_binary_release(
            &prep.config,
            ReleaseTarget::LinuxX86_64,
        )?,
        macos_setup: resolve_mise_setup_for_consumer_binary_release(
            &prep.config,
            ReleaseTarget::MacosArm64,
        )?,
        install_tools_argv: install_argv(&[PinnedTool::Rust, PinnedTool::Gh], &catalog)?,
        install_rust_argv: install_argv(&[PinnedTool::Rust], &catalog)?,
        metadata_argv: metadata_argv(&config.manifest_path, &catalog)?,
        add_target_argv: target_argv(&catalog)?,
        build_argv: build_argv(config, &catalog)?,
        gh_argv: exec_argv(&[PinnedTool::Gh], "gh", &[], &catalog)?,
    };
    Ok(vec![render_consumer_binary_release(&spec)?])
}

fn require_tracked_lockfile(
    prep: &GenerationPreparation,
    config: &RustBinaryReleaseConfig,
) -> Result<(), OrchestratorError> {
    let workspace = Path::new(&config.manifest_path)
        .parent()
        .and_then(Path::to_str)
        .filter(|path| *path != ".")
        .unwrap_or("");
    let relative = if workspace.is_empty() {
        "Cargo.lock".to_owned()
    } else {
        format!("{workspace}/Cargo.lock")
    };
    let lockfile = prep.root.join(&relative);
    match fs::symlink_metadata(&lockfile) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(OrchestratorError::Contract {
                problem: format!("binary_release_lockfile_symlink:{relative}"),
            });
        }
        Ok(_) => {
            return Err(OrchestratorError::Contract {
                problem: format!("binary_release_lockfile_not_regular:{relative}"),
            });
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(OrchestratorError::Contract {
                problem: format!("binary_release_lockfile_missing:{relative}"),
            });
        }
        Err(err) => {
            return Err(OrchestratorError::io(
                lockfile.display().to_string(),
                err.to_string(),
            ));
        }
    }
    let request = GitRequest::ls_files(vec![
        "--error-unmatch".into(),
        "--".into(),
        relative.clone().into(),
    ]);
    let output = request.run_in(&prep.root).map_err(contract_error)?;
    if !output.success {
        return Err(OrchestratorError::Contract {
            problem: format!("binary_release_lockfile_untracked:{relative}"),
        });
    }
    Ok(())
}

fn enabled_config(
    prep: &GenerationPreparation,
) -> Result<Option<&RustBinaryReleaseConfig>, OrchestratorError> {
    let Some(config) = prep
        .config
        .stacks
        .rust
        .as_ref()
        .map(|rust| &rust.binary_release)
    else {
        return Ok(None);
    };
    if !config.enabled {
        return Ok(None);
    }
    if prep.config.workflow.policy != WorkflowPolicy::ConsumerV1 {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.binary_release.enabled",
            "binary_release_requires_consumer_policy",
        ));
    }
    config
        .validate(CONFIG_REL)
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    Ok(Some(config))
}

fn selected_package<'a>(
    prep: &'a GenerationPreparation,
    config: &RustBinaryReleaseConfig,
) -> Result<&'a PackageRecord, OrchestratorError> {
    let workspace_root = Path::new(&config.manifest_path)
        .parent()
        .and_then(Path::to_str)
        .filter(|path| *path != ".")
        .unwrap_or("");
    let mut workspaces = prep
        .discovery
        .workspaces
        .iter()
        .map(|planned| &planned.record)
        .filter(|record| record.workspace_root == workspace_root);
    let workspace = workspaces
        .next()
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("binary_release_workspace_not_discovered:{workspace_root}"),
        })?;
    if workspaces.next().is_some() {
        return Err(OrchestratorError::Contract {
            problem: "binary_release_workspace_ambiguous".to_owned(),
        });
    }
    let mut packages = workspace.packages.iter().filter(|package| {
        package.in_workspace && !package.external && package.name == config.package
    });
    let package = packages.next().ok_or_else(|| OrchestratorError::Contract {
        problem: format!("binary_release_package_not_discovered:{}", config.package),
    })?;
    if packages.next().is_some() {
        return Err(OrchestratorError::Contract {
            problem: "binary_release_package_ambiguous".to_owned(),
        });
    }
    let mut binaries = package
        .targets
        .iter()
        .filter(|target| target.kind == "bin" && target.name == config.bin);
    let binary = binaries.next().ok_or_else(|| OrchestratorError::Contract {
        problem: format!("binary_release_target_not_discovered:{}", config.bin),
    })?;
    if binaries.next().is_some() {
        return Err(OrchestratorError::Contract {
            problem: "binary_release_target_ambiguous".to_owned(),
        });
    }
    if !binary.required_features.is_empty() {
        return Err(OrchestratorError::unsupported(
            "binary_required_features",
            "the initial consumer binary release path requires a binary without required-features",
        ));
    }
    Ok(package)
}

fn install_argv(
    tools: &[PinnedTool],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let install = MiseInstall::new(tools.to_vec()).map_err(contract_error)?;
    strings_of(install.argv(catalog)).map_err(contract_error)
}

fn metadata_argv(manifest: &str, catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    exec_argv(
        &[PinnedTool::Rust],
        "cargo",
        &[
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--no-deps",
            "--manifest-path",
            manifest,
        ],
        catalog,
    )
}

fn target_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let request = PrepareRustTarget::new(
        ReleaseTarget::MacosArm64.triple(),
        ReleaseTarget::MacosArm64.triple(),
    )
    .map_err(contract_error)?;
    strings_of(request.argv(catalog)).map_err(contract_error)
}

fn build_argv(
    config: &RustBinaryReleaseConfig,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    exec_argv(
        &[PinnedTool::Rust],
        "cargo",
        &[
            "build",
            "--release",
            "--locked",
            "--manifest-path",
            &config.manifest_path,
            "--package",
            &config.package,
            "--bin",
            &config.bin,
            "--target",
            velnor_actions_contract::config::CONSUMER_BINARY_TARGET,
        ],
        catalog,
    )
}

fn exec_argv(
    tools: &[PinnedTool],
    program: &str,
    args: &[&str],
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let request = PinnedToolExec::new(
        tools.to_vec(),
        OsStr::new(program),
        args.iter().map(OsString::from).collect(),
    )
    .map_err(contract_error)?;
    strings_of(request.argv(catalog)).map_err(contract_error)
}

fn contract_error(problem: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_string(),
    }
}
