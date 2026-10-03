//! Qualified source bytes become retained, observed owned installation trees.
use crate::OrchestratorError;
use crate::check_evidence::gate::tools::{QualifiedToolReceipt, receipt};
use crate::internal::internal;
use std::path::{Path, PathBuf};
use std::time::Instant;
use velnor_actions_contract::config::{
    QualifiedCargoInstallation, QualifiedTool, QualifiedToolBackend, QualifiedToolOptions,
};
use velnor_actions_mise::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedProbeHomes, verify_qualified_executable,
};
use velnor_actions_mise::{DiscoveredCheck, QualifiedCheck};

#[path = "check_tool_archive.rs"]
mod archive;
#[path = "check_tool_layout.rs"]
mod layout;
#[path = "check_tool_tree.rs"]
mod tree;

pub(super) fn acquire(
    handle: &QualifiedCheck,
    check: &DiscoveredCheck,
    home: &Path,
) -> Result<Vec<QualifiedToolReceipt>, OrchestratorError> {
    let started = Instant::now();
    let mut receipts = Vec::new();
    let mut expanded = archive::ArchiveBudget::new();
    for tool in &check.qualified_tools {
        let qualified = tool
            .platforms
            .iter()
            .find(|p| p.platform == check.check.runner.platform)
            .ok_or_else(|| internal("qualified_tool_platform"))?;
        let tool_home = handle
            .qualified_tool_home(&tool.id)
            .map_err(|e| internal(&e.to_string()))?;
        for directory in ["downloads", "verified", "unpacked", "cargo", "target"] {
            crate::exclusive_write::create_dir_no_symlink(home, &tool_home.join(directory))?;
        }
        let primary = fetch_extract(
            handle,
            tool,
            false,
            &qualified.artifacts,
            &tool_home,
            started,
            &mut expanded,
        )?;
        let dependencies = fetch_extract(
            handle,
            tool,
            true,
            &qualified.dependency_artifacts,
            &tool_home,
            started,
            &mut expanded,
        )?;
        let prefix = tool_home.join("prefix");
        if source_mode(tool) {
            layout::prepare_cargo_source(
                tool,
                check.check.runner.platform,
                &tool_home,
                &primary,
                &dependencies,
            )?;
            run(handle, handle.qualified_source_command(&tool.id), started)?;
        } else {
            layout::normalize_payload(tool, check.check.runner.platform, &primary, &prefix)?;
        }
        tree::freeze_tree(&prefix)?;
        let actual_tree = tree::tree_sha256(&prefix)?;
        if actual_tree != qualified.install_tree_sha256 {
            return Err(internal("qualified_tool_install_tree_sha256"));
        }
        let proofs = observe_executables(
            tool,
            check,
            home,
            &prefix,
            compiler_prefix(tool, &check.qualified_tools, home),
        )?;
        receipts.push(receipt(
            tool,
            check.check.runner.platform,
            qualified.artifacts.clone(),
            qualified.dependency_artifacts.clone(),
            actual_tree,
            proofs,
        )?);
        run(handle, handle.qualified_link_command(&tool.id), started)?;
    }
    Ok(receipts)
}

fn source_mode(tool: &QualifiedTool) -> bool {
    matches!(
        &tool.options,
        QualifiedToolOptions::Cargo {
            installation: QualifiedCargoInstallation::Source { .. },
            ..
        }
    )
}

fn fetch_extract(
    handle: &QualifiedCheck,
    tool: &QualifiedTool,
    dependency: bool,
    artifacts: &[velnor_actions_contract::config::QualifiedToolArtifact],
    tool_home: &Path,
    started: Instant,
    expanded: &mut archive::ArchiveBudget,
) -> Result<Vec<PathBuf>, OrchestratorError> {
    let mut roots = Vec::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        run(
            handle,
            handle.qualified_fetch_command(&tool.id, dependency, index),
            started,
        )?;
        let downloaded = tool_home.join("downloads").join(&artifact.sha256);
        let bytes = crate::retrieve_reports::read_staged_bytes(&downloaded, 1024 * 1024 * 1024)
            .map_err(|_| internal("qualified_tool_artifact_unreadable"))?;
        if crate::cover_identity::generator::sha256_hex(&bytes) != artifact.sha256 {
            return Err(internal("qualified_tool_artifact_sha256"));
        }
        let name = format!(
            "{}-{index}",
            if dependency { "dependency" } else { "primary" }
        );
        let verified = tool_home.join("verified").join(&name);
        crate::exclusive_write::write_exclusive(&verified, &bytes, "qualified_tool_archive")?;
        let root = tool_home.join("unpacked").join(name);
        archive::extract_archive(&verified, &root, &artifact.url, expanded)?;
        std::fs::remove_file(&downloaded)
            .map_err(|_| internal("qualified_tool_archive_cleanup"))?;
        std::fs::remove_file(&verified).map_err(|_| internal("qualified_tool_archive_cleanup"))?;
        roots.push(root);
    }
    Ok(roots)
}

fn compiler_prefix(tool: &QualifiedTool, tools: &[QualifiedTool], home: &Path) -> Option<PathBuf> {
    let compiler = match &tool.backend {
        QualifiedToolBackend::Core { tool: name } if name == "rust" => Some(tool),
        _ if tool.requires_compiler() => tools.iter().find(|candidate| {
            tool.depends_on.contains(&candidate.id)
                && matches!(&candidate.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
        }),
        _ => None,
    };
    compiler.map(|tool| home.join("tools").join(&tool.id).join("prefix"))
}

fn observe_executables(
    tool: &QualifiedTool,
    check: &DiscoveredCheck,
    home: &Path,
    prefix: &Path,
    compiler_toolchain: Option<PathBuf>,
) -> Result<Vec<velnor_actions_mise::check_tool_probes::QualifiedExecutableProof>, OrchestratorError>
{
    let qualified = tool
        .platforms
        .iter()
        .find(|p| p.platform == check.check.runner.platform)
        .ok_or_else(|| internal("qualified_tool_platform"))?;
    let homes = QualifiedProbeHomes {
        home: home.into(),
        bin_dir: home.join("bin"),
        cargo_home: home.join("cargo"),
        rust_home: home.join(velnor_actions_mise::checks::RUSTUP_HOME_SUFFIX),
        compiler_toolchain,
    };
    let mut proofs = Vec::new();
    for executable in &qualified.executables {
        crate::check_evidence::reject_link_components(prefix, &executable.path)?;
        let path = prefix.join(&executable.path);
        let bytes = crate::retrieve_reports::read_staged_bytes(&path, 256 * 1024 * 1024)
            .map_err(|_| internal("qualified_tool_executable_unreadable"))?;
        let sha256 = crate::cover_identity::generator::sha256_hex(&bytes);
        if sha256 != executable.sha256 {
            return Err(internal("qualified_tool_executable_sha256"));
        }
        let observed = QualifiedExecutableObservation {
            name: executable.name.clone(),
            path: path.clone(),
            sha256,
        };
        let proof = verify_qualified_executable(
            tool,
            check.check.runner.platform,
            executable,
            &observed,
            &homes,
        )
        .map_err(|e| internal(&e.to_string()))?;
        super::link_program(path.as_os_str(), &home.join("bin").join(&executable.name))?;
        proofs.push(proof);
    }
    Ok(proofs)
}

fn run(
    handle: &QualifiedCheck,
    command: Result<velnor_actions_mise::IsolatedCommand, velnor_actions_mise::MiseError>,
    started: Instant,
) -> Result<(), OrchestratorError> {
    let timeout = handle
        .timeout()
        .checked_sub(started.elapsed())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| internal("check_timeout"))?;
    let output = command
        .map_err(|e| internal(&e.to_string()))?
        .run_bounded(8 * 1024 * 1024, timeout)
        .map_err(|e| internal(&e.to_string()))?;
    output
        .require_success("qualified-tool-acquisition")
        .map_err(|e| internal(&e.to_string()))?;
    Ok(())
}

#[cfg(test)]
#[path = "check_tool_acquire_tests.rs"]
mod tests;
