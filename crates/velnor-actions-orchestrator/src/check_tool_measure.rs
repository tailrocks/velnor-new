//! Bounded collection of untrusted tool identity candidates for pin promotion.
use crate::OrchestratorError;
use crate::check_evidence::gate::tools::{MeasuredToolExecutable, UntrustedToolMeasurement};
use crate::check_evidence::reject_link_components;
use crate::internal::internal;
use std::path::{Path, PathBuf};
use velnor_actions_contract::config::{
    CheckPlatform, MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES, QualifiedCargoInstallation,
    QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend, QualifiedToolExecutable,
    QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe,
};
use velnor_actions_mise::CheckDeadline;
use velnor_actions_mise::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedProbeHomes, verify_qualified_executable,
};

use super::{archive, check_deadline, freeze_and_hash_tree, sha256_with_deadline};

const MEASUREMENT_SCHEMA: &str = "velnor.qualified-tool-measurement-candidate.v1";
// Reused only to validate the source-only request shape; never emitted or probed.
const VALIDATION_DIGEST: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const MAX_MEASUREMENT_ARCHIVES: usize = 32;
const MAX_MEASUREMENT_EXECUTABLES: usize = 32;
const MAX_MEASUREMENT_DEPENDENCIES: usize = 32;
const MAX_MEASUREMENT_OPTION_ITEMS: usize = 64;
const MAX_MEASUREMENT_URL_BYTES: usize = 2048;

/// Tool identity and source declaration without installed-byte digests.
#[derive(Debug, Clone)]
pub struct ToolMeasurementDeclaration {
    /// Stable repository tool identity.
    pub id: String,
    /// Supported archive backend.
    pub backend: QualifiedToolBackend,
    /// Exact three-part release version.
    pub version: String,
    /// Backend-specific install options.
    pub options: QualifiedToolOptions,
    /// Declared direct prerequisite identities.
    pub depends_on: Vec<String>,
    /// Executable paths and exact expected version-probe output.
    pub executables: Vec<MeasurementExecutableDeclaration>,
}

/// Exact executable location and fixed probe expected for the declared version.
#[derive(Debug, Clone)]
pub struct MeasurementExecutableDeclaration {
    /// Stable executable name.
    pub name: String,
    /// Relative path below the normalized install prefix.
    pub path: String,
    /// Fixed probe and exact expected output for this release.
    pub probe: QualifiedToolProbe,
}

/// Trusted archive source and a locally staged archive to measure.
#[derive(Debug, Clone)]
pub struct MeasurementArchiveInput {
    /// Declared source URL and expected source archive SHA-256.
    pub artifact: QualifiedToolArtifact,
    /// Local path to the bytes that must match the declared archive hash.
    pub archive_path: PathBuf,
}

/// One-platform measurement request; installed-tree and executable digests are absent.
#[derive(Debug, Clone)]
pub struct ToolMeasurementRequest {
    /// Candidate identity and expected probe declarations.
    pub declaration: ToolMeasurementDeclaration,
    /// Native platform being measured.
    pub platform: CheckPlatform,
    /// Declared source archives and their locally staged bytes.
    pub artifacts: Vec<MeasurementArchiveInput>,
    /// Direct Rust prerequisite ID for probes whose declaration requires a compiler.
    pub compiler_tool_id: Option<String>,
}

/// Measure an explicit source declaration without creating a receipt or runtime links.
///
/// The candidate contains external probe output and observed installed digests only.
/// It is not an admission result and does not modify repository configuration.
///
/// # Errors
/// Rejects non-native platforms, invalid declarations, source-hash mismatches,
/// unsafe or oversized archives, unsupported layouts, and failed version probes.
pub fn measure_tool_candidate(
    request: &ToolMeasurementRequest,
    home: &Path,
    deadline: CheckDeadline,
) -> Result<UntrustedToolMeasurement, OrchestratorError> {
    if std::env::consts::OS != request.platform.os()
        || std::env::consts::ARCH != request.platform.arch()
    {
        return Err(internal("qualified_tool_measurement_platform_unsupported"));
    }
    check_deadline(deadline)?;
    let home = canonical_home(home)?;
    let unmeasured = validate_request(request)?;
    let tool_home = prepare_measurement_home(&home, &request.declaration.id)?;
    let mut expanded = archive::ArchiveBudget::new();
    let roots = stage_and_extract(&request.artifacts, &tool_home, &mut expanded, deadline)?;
    let prefix = normalize_payload(&unmeasured, request.platform, &roots, &tool_home, deadline)?;
    let measured_tree = freeze_and_hash_tree(&prefix, deadline)?;
    let executable_hashes =
        measure_executable_hashes(&request.declaration.executables, &prefix, deadline)?;
    let measured_tool = request.qualified_tool(&measured_tree, Some(&executable_hashes))?;
    measured_tool
        .validate("qualified_tool_measurement", &request.declaration.id)
        .map_err(crate::internal::internal_contract)?;
    let compiler_toolchain = compiler_toolchain(request, &measured_tool, &home, &prefix)?;
    let homes = probe_homes(&home, &tool_home, compiler_toolchain);
    let executable_observations =
        measure_executable_probes(&measured_tool, request.platform, &prefix, &homes, deadline)?;

    Ok(candidate(
        &measured_tool,
        request.platform,
        measured_tree,
        executable_observations,
    ))
}

impl ToolMeasurementRequest {
    fn qualified_tool(
        &self,
        tree_sha256: &str,
        executable_sha256: Option<&[String]>,
    ) -> Result<QualifiedTool, OrchestratorError> {
        if executable_sha256
            .is_some_and(|hashes| hashes.len() != self.declaration.executables.len())
        {
            return Err(internal("qualified_tool_measurement_executable_count"));
        }
        let executables = self
            .declaration
            .executables
            .iter()
            .enumerate()
            .map(|(index, executable)| QualifiedToolExecutable {
                name: executable.name.clone(),
                path: executable.path.clone(),
                sha256: executable_sha256
                    .and_then(|hashes| hashes.get(index))
                    .cloned()
                    .unwrap_or_else(|| VALIDATION_DIGEST.to_owned()),
                probe: executable.probe.clone(),
            })
            .collect();
        Ok(QualifiedTool {
            id: self.declaration.id.clone(),
            backend: self.declaration.backend.clone(),
            version: self.declaration.version.clone(),
            options: self.declaration.options.clone(),
            depends_on: self.declaration.depends_on.clone(),
            platforms: vec![QualifiedToolPlatform {
                platform: self.platform,
                artifacts: self
                    .artifacts
                    .iter()
                    .map(|input| input.artifact.clone())
                    .collect(),
                dependency_artifacts: vec![],
                install_tree_sha256: tree_sha256.to_owned(),
                executables,
            }],
        })
    }
}

fn canonical_home(home: &Path) -> Result<PathBuf, OrchestratorError> {
    let metadata =
        std::fs::symlink_metadata(home).map_err(|_| internal("qualified_tool_measurement_home"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(internal("qualified_tool_measurement_home"));
    }
    home.canonicalize()
        .map_err(|_| internal("qualified_tool_measurement_home"))
}

fn validate_request(request: &ToolMeasurementRequest) -> Result<QualifiedTool, OrchestratorError> {
    if request.artifacts.is_empty()
        || request.artifacts.len() > MAX_MEASUREMENT_ARCHIVES
        || request.declaration.executables.is_empty()
        || request.declaration.executables.len() > MAX_MEASUREMENT_EXECUTABLES
        || request.declaration.depends_on.len() > MAX_MEASUREMENT_DEPENDENCIES
        || !bounded_options(&request.declaration.options)
        || request.artifacts.iter().any(|input| {
            input.artifact.url.len() > MAX_MEASUREMENT_URL_BYTES
                || input.archive_path.as_os_str().len() > MAX_MEASUREMENT_URL_BYTES
        })
        || request.declaration.executables.iter().any(|executable| {
            executable.name.len() > 128
                || executable.path.len() > 512
                || probe_expected_bytes(&executable.probe)
                    > MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES
        })
    {
        return Err(internal("qualified_tool_measurement_input_limit"));
    }
    if matches!(
        &request.declaration.options,
        QualifiedToolOptions::Cargo {
            installation: QualifiedCargoInstallation::Source { .. },
            ..
        }
    ) {
        return Err(internal(
            "qualified_tool_measurement_cargo_source_unsupported",
        ));
    }
    let unmeasured = request.qualified_tool(VALIDATION_DIGEST, None)?;
    unmeasured
        .validate("qualified_tool_measurement", &request.declaration.id)
        .map_err(crate::internal::internal_contract)?;
    validate_compiler_binding(request, &unmeasured)?;
    validate_archive_inputs(&request.artifacts, &unmeasured.platforms[0].artifacts)?;
    Ok(unmeasured)
}

fn bounded_options(options: &QualifiedToolOptions) -> bool {
    match options {
        QualifiedToolOptions::Default => true,
        QualifiedToolOptions::Rust {
            components,
            targets,
        } => {
            components.len() <= MAX_MEASUREMENT_OPTION_ITEMS
                && targets.len() <= MAX_MEASUREMENT_OPTION_ITEMS
        }
        QualifiedToolOptions::Cargo { features, .. } => {
            features.len() <= MAX_MEASUREMENT_OPTION_ITEMS
        }
    }
}

fn probe_expected_bytes(probe: &QualifiedToolProbe) -> usize {
    match probe {
        QualifiedToolProbe::Version { expected }
        | QualifiedToolProbe::VersionSubcommand { expected }
        | QualifiedToolProbe::RustcVerbose { expected }
        | QualifiedToolProbe::CargoNextestVersion { expected } => expected.len(),
    }
}

fn prepare_measurement_home(home: &Path, tool_id: &str) -> Result<PathBuf, OrchestratorError> {
    let tool_home = home.join("tools").join(tool_id);
    crate::exclusive_write::create_dir_no_symlink(home, &home.join("tools"))?;
    std::fs::create_dir(&tool_home)
        .map_err(|_| internal("qualified_tool_measurement_home_exists"))?;
    for directory in [
        "verified",
        "unpacked",
        "cargo",
        "target",
        "probe-bin",
        "rustup",
    ] {
        crate::exclusive_write::create_dir_no_symlink(home, &tool_home.join(directory))?;
    }
    Ok(tool_home)
}

fn normalize_payload(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    roots: &[PathBuf],
    tool_home: &Path,
    deadline: CheckDeadline,
) -> Result<PathBuf, OrchestratorError> {
    let prefix = tool_home.join("prefix");
    super::layout::normalize_payload(tool, platform, roots, &prefix, deadline)?;
    check_deadline(deadline)?;
    prefix
        .canonicalize()
        .map_err(|_| internal("qualified_tool_measurement_prefix"))
}

fn probe_homes(
    home: &Path,
    tool_home: &Path,
    compiler_toolchain: Option<PathBuf>,
) -> QualifiedProbeHomes {
    QualifiedProbeHomes {
        home: home.to_owned(),
        bin_dir: tool_home.join("probe-bin"),
        cargo_home: tool_home.join("cargo"),
        rust_home: tool_home.join("rustup"),
        compiler_toolchain,
    }
}

fn candidate(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    measured_tree: String,
    executable_observations: Vec<MeasuredToolExecutable>,
) -> UntrustedToolMeasurement {
    let qualification = &tool.platforms[0];
    UntrustedToolMeasurement {
        schema: MEASUREMENT_SCHEMA.to_owned(),
        tool_id: tool.id.clone(),
        backend: tool.backend.clone(),
        version: tool.version.clone(),
        options: tool.options.clone(),
        depends_on: tool.depends_on.clone(),
        platform,
        source_artifacts: qualification.artifacts.clone(),
        dependency_source_artifacts: qualification.dependency_artifacts.clone(),
        measured_install_tree_sha256: measured_tree,
        executable_observations,
    }
}

fn validate_compiler_binding(
    request: &ToolMeasurementRequest,
    tool: &QualifiedTool,
) -> Result<(), OrchestratorError> {
    let rust_tool = matches!(
        &tool.backend,
        QualifiedToolBackend::Core { tool } if tool == "rust"
    );
    if rust_tool {
        if request.compiler_tool_id.is_some() {
            return Err(internal("qualified_tool_measurement_unexpected_compiler"));
        }
    } else if tool.requires_compiler() {
        let Some(compiler_id) = request.compiler_tool_id.as_deref() else {
            return Err(internal("qualified_tool_measurement_compiler_required"));
        };
        if !tool
            .depends_on
            .iter()
            .any(|dependency| dependency == compiler_id)
        {
            return Err(internal("qualified_tool_measurement_compiler_dependency"));
        }
    } else if request.compiler_tool_id.is_some() {
        return Err(internal("qualified_tool_measurement_unexpected_compiler"));
    }
    Ok(())
}

fn validate_archive_inputs(
    inputs: &[MeasurementArchiveInput],
    declarations: &[QualifiedToolArtifact],
) -> Result<(), OrchestratorError> {
    if inputs.len() != declarations.len()
        || inputs
            .iter()
            .zip(declarations)
            .any(|(input, declaration)| &input.artifact != declaration)
    {
        return Err(internal("qualified_tool_measurement_archive_binding"));
    }
    Ok(())
}

fn stage_and_extract(
    inputs: &[MeasurementArchiveInput],
    tool_home: &Path,
    expanded: &mut archive::ArchiveBudget,
    deadline: CheckDeadline,
) -> Result<Vec<PathBuf>, OrchestratorError> {
    let mut roots = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        check_deadline(deadline)?;
        let stem = format!("primary-{index}");
        let verified = tool_home.join("verified").join(&stem);
        let destination = tool_home.join("unpacked").join(&stem);
        super::verify_and_extract(
            &input.archive_path,
            &verified,
            &destination,
            &input.artifact,
            expanded,
            deadline,
        )?;
        roots.push(destination);
    }
    Ok(roots)
}

fn measure_executable_hashes(
    executables: &[MeasurementExecutableDeclaration],
    prefix: &Path,
    deadline: CheckDeadline,
) -> Result<Vec<String>, OrchestratorError> {
    let mut hashes = Vec::with_capacity(executables.len());
    for executable in executables {
        check_deadline(deadline)?;
        reject_link_components(prefix, &executable.path)?;
        let path = prefix.join(&executable.path);
        let bytes = super::read_with_deadline(&path, 256 * 1024 * 1024, deadline)
            .map_err(|_| internal("qualified_tool_measurement_executable_unreadable"))?;
        hashes.push(sha256_with_deadline(&bytes, deadline)?);
    }
    Ok(hashes)
}

fn compiler_toolchain(
    request: &ToolMeasurementRequest,
    tool: &QualifiedTool,
    home: &Path,
    prefix: &Path,
) -> Result<Option<PathBuf>, OrchestratorError> {
    if matches!(&tool.backend, QualifiedToolBackend::Core { tool } if tool == "rust") {
        return Ok(Some(prefix.to_owned()));
    }
    if !tool.requires_compiler() {
        return Ok(None);
    }
    let compiler_id = request
        .compiler_tool_id
        .as_deref()
        .ok_or_else(|| internal("qualified_tool_measurement_compiler_required"))?;
    let prefix = home.join("tools").join(compiler_id).join("prefix");
    let rustc = prefix.join("bin/rustc");
    let rustc_metadata = std::fs::symlink_metadata(&rustc)
        .map_err(|_| internal("qualified_tool_measurement_compiler_prefix"))?;
    if !rustc_metadata.is_file() || rustc_metadata.file_type().is_symlink() {
        return Err(internal("qualified_tool_measurement_compiler_prefix"));
    }
    let canonical = prefix
        .canonicalize()
        .map_err(|_| internal("qualified_tool_measurement_compiler_prefix"))?;
    if canonical != prefix || !canonical.starts_with(home) {
        return Err(internal("qualified_tool_measurement_compiler_prefix"));
    }
    Ok(Some(canonical))
}

fn measure_executable_probes(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    prefix: &Path,
    homes: &QualifiedProbeHomes,
    deadline: CheckDeadline,
) -> Result<Vec<MeasuredToolExecutable>, OrchestratorError> {
    let qualified = tool
        .platforms
        .iter()
        .find(|candidate| candidate.platform == platform)
        .ok_or_else(|| internal("qualified_tool_measurement_platform"))?;
    let mut observations = Vec::with_capacity(qualified.executables.len());
    for executable in &qualified.executables {
        check_deadline(deadline)?;
        reject_link_components(prefix, &executable.path)?;
        let path = prefix.join(&executable.path);
        let observed = QualifiedExecutableObservation {
            name: executable.name.clone(),
            path: path
                .canonicalize()
                .map_err(|_| internal("qualified_tool_measurement_executable_path"))?,
            sha256: executable.sha256.clone(),
        };
        let proof =
            verify_qualified_executable(tool, platform, executable, &observed, homes, deadline)
                .map_err(|error| internal(&error.to_string()))?;
        observations.push(MeasuredToolExecutable {
            name: executable.name.clone(),
            path: executable.path.clone(),
            measured_sha256: executable.sha256.clone(),
            probe: executable.probe.clone(),
            stdout: proof.stdout,
            stdout_digest: proof.stdout_digest,
            stderr_digest: proof.stderr_digest,
        });
    }
    Ok(observations)
}

#[cfg(test)]
#[path = "check_tool_measure_tests.rs"]
mod tests;
