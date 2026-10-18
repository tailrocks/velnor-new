//! Runtime identity evidence and exact cache key binding.

use serde::{Deserialize, Serialize};

use super::receipt::{QualificationCacheBackendEntry, QualificationCacheBackendObservation};
use crate::cachekey::ToolchainInputs;
use crate::canonical::{canonical_json_bytes, digest_b3, validate_digest};
use crate::errors::ContractError;

/// Runtime fact a cache producer must collect before binding a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationRuntimeIdentityField {
    /// Actual hosted runner OS.
    RunnerOs,
    /// Actual hosted runner architecture.
    RunnerArch,
    /// Actual hosted image or verified Scale Set image identity.
    ImageIdentity,
    /// Toolchain identity observed after pinned tool setup.
    ToolchainId,
    /// Cache-format identity reported by its producer.
    CacheFormatId,
    /// ABI identity reported by the actual runner and toolchain.
    AbiId,
    /// Selected compile-driver/configuration identity.
    DriverId,
}

/// Digests for one verified container image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeImageDigests {
    /// SHA-256 digest of the source archive, as lowercase hex.
    pub archive_sha256: String,
    /// SHA-256 hash of the archive's outer `index.json`, as lowercase hex.
    pub archive_index_digest: String,
    /// Digest of a nested selected OCI image index, when one is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_index_digest: Option<String>,
    /// OCI manifest digest in `sha256:<hex>` form.
    pub manifest_digest: String,
    /// OCI config digest in `sha256:<hex>` form.
    pub config_digest: String,
}

/// Verified Scale Set labels and name published by the runner host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeScaleSet {
    /// Registered Scale Set name.
    pub name: String,
    /// Exact Scale Set selector labels in render order.
    pub labels: Vec<String>,
}

/// Image facts supplied by GitHub or the verified runner host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualificationRuntimeImageIdentity {
    /// Image family and version supplied by a GitHub-hosted runner.
    Hosted {
        /// Hosted image family.
        image_os: String,
        /// Hosted image version.
        image_version: String,
    },
    /// Image data from the runner host's read-only guest projection.
    ScaleSet {
        /// Source revision used to build the verified guest images.
        source_sha: String,
        /// OCI guest platform, such as `linux/amd64`.
        guest_platform: String,
        /// Scale Set selector values verified by the runner host.
        scale_set: QualificationRuntimeScaleSet,
        /// Verified runner image digests.
        runner_image: QualificationRuntimeImageDigests,
        /// Verified Docker-in-Docker image digests.
        dind_image: QualificationRuntimeImageDigests,
    },
}

/// Runtime platform dimensions reported by the active runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimePlatform {
    /// Operating system name.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// Logical runner label from the plan.
    pub runner_label: String,
    /// Exact hosted label or canonical Scale Set selector.
    pub runs_on: String,
    /// Actual image facts for the selected runner.
    pub image_identity: QualificationRuntimeImageIdentity,
    /// Execution target (`host` or a triple).
    pub target: String,
}

/// One tool version and executable digest observed after trusted setup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeToolObservation {
    /// Exact pinned selector from the plan.
    pub selector: String,
    /// Version reported by the resolved executable.
    pub reported_version: String,
    /// SHA-256 of the resolved executable bytes, as lowercase hex.
    pub executable_sha256: String,
}

/// ABI or runtime facts collected from the selected host and toolchain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualificationRuntimeAbiObservation {
    /// Host kernel, architecture and C runtime facts.
    Host {
        /// Operating-system name reported by the host.
        os: String,
        /// Architecture reported by the host.
        arch: String,
        /// Kernel release reported by the host.
        kernel: String,
        /// C runtime release, when the operating system has one.
        libc: Option<String>,
    },
    /// Rust compiler host, target and cfg facts.
    Rust {
        /// Rust compiler's host triple.
        host: String,
        /// Exact selected compilation target.
        target: String,
        /// Output from `rustc --version --verbose`.
        version_verbose: String,
        /// Output from `rustc --print cfg --target <target>`.
        target_cfg: String,
        /// Installed Rustup components reported by `rustup component list`.
        installed_components: Vec<String>,
    },
    /// Java runtime version and machine architecture.
    Java {
        /// Runtime version output.
        version_output: String,
        /// Machine architecture reported by the runtime.
        arch: String,
    },
    /// JavaScript runtime and native module ABI facts.
    Node {
        /// Runtime version output.
        version_output: String,
        /// Native module ABI version.
        module_abi: String,
        /// N-API version.
        napi: String,
        /// Operating-system platform reported by the runtime.
        platform: String,
        /// Machine architecture reported by the runtime.
        arch: String,
    },
    /// OpenTofu runtime version and platform facts.
    OpenTofu {
        /// Version output.
        version_output: String,
        /// Operating-system and architecture reported by OpenTofu.
        platform: String,
    },
}

/// Runtime facts needed to bind a logical slot to one exact cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeIdentity {
    /// Runtime OS, architecture, label, image and target evidence.
    pub platform: QualificationRuntimePlatform,
    /// Exact tool versions and executable bytes observed after trusted setup.
    pub tools: Vec<QualificationRuntimeToolObservation>,
    /// ABI facts reported by the host and selected runtime adapters.
    pub abi: Vec<QualificationRuntimeAbiObservation>,
}

/// Plan-known cache identity values plus runtime facts still required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRuntimeIdentityRequirements {
    /// Logical runner label from the plan.
    pub runner_label: String,
    /// Exact hosted label or Scale Set selector from the plan lane.
    pub runs_on: String,
    /// Expected Velnor source revision for a Scale Set guest image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_set_image_source_sha: Option<String>,
    /// Execution target associated with that runner label.
    pub target: String,
    /// Toolchain digest recorded by the matrix entry.
    pub toolchain_id: String,
    /// Exact pins and driver inputs behind `toolchain_id`.
    pub toolchain_inputs: ToolchainInputs,
    /// Cache-format digest recorded by the matrix entry.
    pub cache_format_id: String,
    /// ABI identity must be present and match the admitted predecessor.
    pub require_abi: bool,
    /// Selected driver/configuration digest from the matrix entry.
    pub driver_id: String,
    /// Fields that must be present in runtime evidence before cache access.
    pub required_fields: Vec<QualificationRuntimeIdentityField>,
}

/// Required cache object for one restore request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRestoreExpectation {
    /// The exact primary key submitted to the action.
    pub requested_key: String,
    /// Expected immutable cache object, or None when a miss is required.
    pub expected_cache: Option<QualificationCacheBackendEntry>,
}

/// Exact bound keys emitted only after runtime identity verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundQualificationCacheKeys {
    /// Restore request and required post-action admission evidence.
    pub restore: Option<QualificationCacheRestoreExpectation>,
    /// Key to save after comparing actual layer-state digest.
    pub save_key: Option<String>,
    /// Whether saving is allowed only when state changed.
    pub save_if_state_changes: bool,
    /// Admitted predecessor layer-state digest, if present.
    pub expected_prior_state_digest: Option<String>,
    /// Digest of the observed runtime identity included in each key.
    pub runtime_identity_digest: Option<String>,
}

impl BoundQualificationCacheKeys {
    /// Admit a staged restore only after its actual match and cache object agree.
    ///
    /// Cache actions can restore and extract a prefix match even when their
    /// restore-key list is empty. Restore into an isolated staging directory,
    /// call this method, and discard staged bytes on error before import/use.
    /// # Errors
    pub fn validate_restore_observation(
        &self,
        matched_key: Option<&str>,
        observed: &QualificationCacheBackendObservation,
    ) -> Result<(), ContractError> {
        let Some(expectation) = &self.restore else {
            return if matched_key.is_none()
                && *observed == QualificationCacheBackendObservation::NotQueried
            {
                Ok(())
            } else {
                Err(ContractError::identity(
                    "qualification.cache_restore",
                    "unexpected_restore_observation",
                ))
            };
        };
        match (&expectation.expected_cache, observed) {
            (None, QualificationCacheBackendObservation::Absent) if matched_key.is_none() => Ok(()),
            (Some(expected), QualificationCacheBackendObservation::Found(actual))
                if matched_key == Some(expectation.requested_key.as_str())
                    && actual == expected
                    && actual.key == expectation.requested_key =>
            {
                Ok(())
            }
            _ => Err(ContractError::identity(
                "qualification.cache_restore",
                "actual_match_not_admitted",
            )),
        }
    }

    /// Return the save key only when the observed state meets the phase policy.
    /// # Errors
    pub fn save_key_for_state(&self, state_digest: &str) -> Result<Option<&str>, ContractError> {
        validate_digest(state_digest)?;
        let Some(key) = self.save_key.as_deref() else {
            return Ok(None);
        };
        if self.save_if_state_changes
            && self.expected_prior_state_digest.as_deref() == Some(state_digest)
        {
            return Ok(None);
        }
        Ok(Some(key))
    }
}

/// Check runtime observations against plan identity and derive their digest.
pub(crate) fn validate_runtime_identity(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<String, ContractError> {
    use crate::cachekey::validate_semantic_text;

    let platform = &evidence.platform;
    if platform.runner_label != requirements.runner_label
        || platform.runs_on != requirements.runs_on
        || platform.target != requirements.target
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity",
            "runner_binding_mismatch",
        ));
    }
    for (field, value) in [
        ("runner_os", platform.os.as_str()),
        ("runner_arch", platform.arch.as_str()),
        ("runner_label", platform.runner_label.as_str()),
        ("runs_on", platform.runs_on.as_str()),
        ("target", platform.target.as_str()),
    ] {
        validate_semantic_text(field, value)?;
    }
    validate_image_identity(requirements, platform)?;
    if crate::cachekey::toolchain_id(&requirements.toolchain_inputs)? != requirements.toolchain_id
        || !requirements.require_abi
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity",
            "plan_toolchain_commitment_mismatch",
        ));
    }
    validate_digest(&requirements.cache_format_id)?;
    validate_digest(&requirements.driver_id)?;
    validate_tool_observations(requirements, evidence)?;
    validate_abi_observations(requirements, evidence)?;
    #[derive(Serialize)]
    struct RuntimeIdentityDigestInput<'a> {
        requirements: &'a QualificationRuntimeIdentityRequirements,
        observations: &'a QualificationRuntimeIdentity,
    }
    Ok(digest_b3(&canonical_json_bytes(
        &RuntimeIdentityDigestInput {
            requirements,
            observations: evidence,
        },
    )?))
}

fn validate_tool_observations(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    if evidence.tools.len() != requirements.toolchain_inputs.tools.len()
        || requirements
            .toolchain_inputs
            .tools
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || requirements
            .toolchain_inputs
            .components
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.tools",
            "toolchain_inputs_not_canonical",
        ));
    }
    for (expected, observed) in requirements
        .toolchain_inputs
        .tools
        .iter()
        .zip(&evidence.tools)
    {
        let (name, version) = expected.rsplit_once('@').ok_or_else(|| {
            ContractError::identity("qualification.runtime_identity.tools", "bad_plan_selector")
        })?;
        if name.is_empty()
            || version.is_empty()
            || observed.selector != *expected
            || observed.reported_version != version
        {
            return Err(ContractError::identity(
                "qualification.runtime_identity.tools",
                "pinned_tool_observation_mismatch",
            ));
        }
        validate_semantic_text("runtime_tool.selector", &observed.selector)?;
        validate_semantic_text("runtime_tool.version", &observed.reported_version)?;
        validate_sha256_hex(&observed.executable_sha256)?;
    }
    Ok(())
}

fn validate_abi_observations(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    let host = evidence
        .abi
        .iter()
        .find_map(|fact| match fact {
            QualificationRuntimeAbiObservation::Host {
                os,
                arch,
                kernel,
                libc,
            } => Some((os, arch, kernel, libc)),
            _ => None,
        })
        .ok_or_else(|| {
            ContractError::identity("qualification.runtime_identity.abi", "host_fact_missing")
        })?;
    if normalize_os(host.0) != normalize_os(&evidence.platform.os)
        || normalize_arch(host.1) != normalize_arch(&evidence.platform.arch)
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "host_fact_mismatch",
        ));
    }
    validate_semantic_text("runtime_abi.kernel", host.2)?;
    if let Some(libc) = host.3 {
        validate_semantic_text("runtime_abi.libc", libc)?;
    }
    if normalize_os(&evidence.platform.os) == Some("linux")
        && evidence.platform.target.ends_with("-gnu")
        && host.3.is_none()
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "linux_libc_fact_missing",
        ));
    }

    let has_rust = requirements
        .toolchain_inputs
        .tools
        .iter()
        .any(|selector| selector.starts_with("rust@"));
    let rust = evidence.abi.iter().find_map(|fact| match fact {
        QualificationRuntimeAbiObservation::Rust {
            host,
            target,
            version_verbose,
            target_cfg,
            installed_components,
        } => Some((
            host,
            target,
            version_verbose,
            target_cfg,
            installed_components,
        )),
        _ => None,
    });
    if has_rust != rust.is_some() {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "rust_fact_presence_mismatch",
        ));
    }
    if let Some((host, target, version_verbose, target_cfg, installed_components)) = rust {
        let pinned_version = pinned_version_for_family(requirements, "rust")?;
        let expected_host = expected_host_triple(&evidence.platform).ok_or_else(|| {
            ContractError::identity(
                "qualification.runtime_identity.abi",
                "rust_host_platform_unsupported",
            )
        })?;
        let expected_target_arch = requirements
            .target
            .split('-')
            .next()
            .ok_or_else(|| ContractError::identity("runtime_abi.rust_target", "invalid"))?;
        let expected_target_os = target_os_for_triple(&requirements.target)
            .ok_or_else(|| ContractError::identity("runtime_abi.rust_target", "unsupported"))?;
        let target_cfg_lines = target_cfg.lines().collect::<Vec<_>>();
        if target != &requirements.target
            || !exact_one_prefixed_line(version_verbose, "release: ", pinned_version)
            || !exact_one_prefixed_line(version_verbose, "host: ", host)
            || host != expected_host
            || !target_cfg
                .lines()
                .any(|line| line == format!("target_arch=\"{expected_target_arch}\""))
            || !target_cfg
                .lines()
                .any(|line| line == format!("target_os=\"{expected_target_os}\""))
            || target_cfg_lines.windows(2).any(|pair| pair[0] >= pair[1])
            || installed_components
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(ContractError::identity(
                "qualification.runtime_identity.abi",
                "rust_fact_mismatch",
            ));
        }
        validate_semantic_text("runtime_abi.rust_host", host)?;
        validate_multiline_fact("runtime_abi.rustc_verbose", version_verbose)?;
        validate_multiline_fact("runtime_abi.rustc_cfg", target_cfg)?;
        for component in installed_components {
            validate_semantic_text("runtime_abi.rust_component", component)?;
        }
        let required_components: Vec<&str> = requirements
            .toolchain_inputs
            .components
            .iter()
            .map(String::as_str)
            .filter(|component| matches!(*component, "clippy" | "rustfmt"))
            .collect();
        if required_components
            .iter()
            .any(|component| !installed_components.iter().any(|have| have == component))
        {
            return Err(ContractError::identity(
                "qualification.runtime_identity.abi",
                "rust_component_missing",
            ));
        }
    }

    let mut families = std::collections::BTreeSet::new();
    let mut previous_rank = None;
    for fact in &evidence.abi {
        let (family, rank) = match fact {
            QualificationRuntimeAbiObservation::Host { .. } => ("host", 0),
            QualificationRuntimeAbiObservation::Rust { .. } => ("rust", 1),
            QualificationRuntimeAbiObservation::Java { .. } => ("java", 2),
            QualificationRuntimeAbiObservation::Node { .. } => ("node", 3),
            QualificationRuntimeAbiObservation::OpenTofu { .. } => ("open_tofu", 4),
        };
        if !families.insert(family) || previous_rank.is_some_and(|previous| previous >= rank) {
            return Err(ContractError::identity(
                "qualification.runtime_identity.abi",
                "abi_facts_not_canonical",
            ));
        }
        previous_rank = Some(rank);
        match fact {
            QualificationRuntimeAbiObservation::Java {
                version_output,
                arch,
            } => {
                let pinned_version = pinned_version_for_family(requirements, "java")?;
                validate_semantic_text("runtime_abi.java_arch", arch)?;
                validate_multiline_fact("runtime_abi.java_version", version_output)?;
                if !contains_exact_version(version_output, pinned_version)
                    || normalize_arch(arch) != normalize_arch(&evidence.platform.arch)
                {
                    return Err(ContractError::identity(
                        "qualification.runtime_identity.abi",
                        "java_fact_mismatch",
                    ));
                }
            }
            QualificationRuntimeAbiObservation::Node {
                version_output,
                module_abi,
                napi,
                platform,
                arch,
            } => {
                let pinned_version = pinned_version_for_family(requirements, "node")?;
                for (field, value) in [
                    ("runtime_abi.node_module_abi", module_abi),
                    ("runtime_abi.node_napi", napi),
                    ("runtime_abi.node_platform", platform),
                    ("runtime_abi.node_arch", arch),
                ] {
                    validate_semantic_text(field, value)?;
                }
                validate_multiline_fact("runtime_abi.node_version", version_output)?;
                if !contains_exact_version(version_output, pinned_version)
                    || normalize_os(platform) != normalize_os(&evidence.platform.os)
                    || normalize_arch(arch) != normalize_arch(&evidence.platform.arch)
                {
                    return Err(ContractError::identity(
                        "qualification.runtime_identity.abi",
                        "node_fact_mismatch",
                    ));
                }
            }
            QualificationRuntimeAbiObservation::OpenTofu {
                version_output,
                platform,
            } => {
                let pinned_version = pinned_version_for_family(requirements, "open_tofu")?;
                validate_semantic_text("runtime_abi.tofu_platform", platform)?;
                validate_multiline_fact("runtime_abi.tofu_version", version_output)?;
                if !contains_exact_version(version_output, pinned_version)
                    || !platform_matches_target(platform, &requirements.target)
                {
                    return Err(ContractError::identity(
                        "qualification.runtime_identity.abi",
                        "open_tofu_fact_mismatch",
                    ));
                }
            }
            QualificationRuntimeAbiObservation::Host { .. }
            | QualificationRuntimeAbiObservation::Rust { .. } => {}
        }
    }

    require_abi_for_pins(requirements, &families)?;
    Ok(())
}

fn pinned_version_for_family<'a>(
    requirements: &'a QualificationRuntimeIdentityRequirements,
    family: &str,
) -> Result<&'a str, ContractError> {
    let matches = requirements
        .toolchain_inputs
        .tools
        .iter()
        .filter_map(|selector| {
            let (name, version) = selector.rsplit_once('@')?;
            let matches = match family {
                "rust" => name == "rust",
                "java" => name == "java" || name.ends_with("/java"),
                "node" => matches!(name, "node" | "nodejs" | "bun"),
                "open_tofu" => name == "opentofu",
                _ => false,
            };
            matches.then_some(version)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [version] => Ok(*version),
        [] => Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            format!("{family}_tool_pin_missing"),
        )),
        _ => Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            format!("{family}_tool_pin_ambiguous"),
        )),
    }
}

fn normalize_os(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "linux" => Some("linux"),
        "darwin" | "macos" | "mac os" => Some("macos"),
        "windows" | "win32" => Some("windows"),
        _ => None,
    }
}

fn contains_exact_version(output: &str, expected: &str) -> bool {
    output
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | '+'))
        })
        .any(|token| token == expected || token.strip_prefix('v') == Some(expected))
}

fn exact_one_prefixed_line(output: &str, prefix: &str, expected: &str) -> bool {
    let matches = output
        .lines()
        .filter_map(|line| line.strip_prefix(prefix))
        .collect::<Vec<_>>();
    matches.as_slice() == [expected]
}

fn normalize_arch(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "x64" | "x86_64" | "amd64" => Some("x86_64"),
        "arm64" | "aarch64" => Some("aarch64"),
        "x86" | "i386" | "i686" => Some("i686"),
        _ => None,
    }
}

fn expected_host_triple(platform: &QualificationRuntimePlatform) -> Option<&'static str> {
    match (normalize_os(&platform.os)?, normalize_arch(&platform.arch)?) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
        _ => None,
    }
}

fn target_os_for_triple(target: &str) -> Option<&'static str> {
    if target.contains("-linux-") {
        Some("linux")
    } else if target.ends_with("-darwin") {
        Some("macos")
    } else if target.contains("-windows-") {
        Some("windows")
    } else {
        None
    }
}

fn platform_matches_target(platform: &str, target: &str) -> bool {
    let Some(os) = target_os_for_triple(target) else {
        return false;
    };
    let Some(arch) = target.split('-').next().and_then(normalize_arch) else {
        return false;
    };
    platform.to_ascii_lowercase().replace('-', "_")
        == format!(
            "{}_{}",
            os,
            match arch {
                "x86_64" => "amd64",
                "aarch64" => "arm64",
                other => other,
            }
        )
}

fn require_abi_for_pins(
    requirements: &QualificationRuntimeIdentityRequirements,
    families: &std::collections::BTreeSet<&str>,
) -> Result<(), ContractError> {
    let has_java = requirements.toolchain_inputs.tools.iter().any(|selector| {
        selector
            .split('@')
            .next()
            .is_some_and(|name| name.ends_with("/java") || name == "java")
    });
    let has_node = requirements.toolchain_inputs.tools.iter().any(|selector| {
        selector
            .split('@')
            .next()
            .is_some_and(|name| matches!(name, "node" | "nodejs" | "bun"))
    });
    let has_tofu = requirements.toolchain_inputs.compile_driver == "tofu"
        || requirements
            .toolchain_inputs
            .tools
            .iter()
            .any(|selector| selector.starts_with("opentofu@"));
    for (required, family) in [
        (has_java, "java"),
        (has_node, "node"),
        (has_tofu, "open_tofu"),
    ] {
        if required != families.contains(family) {
            return Err(ContractError::identity(
                "qualification.runtime_identity.abi",
                format!(
                    "{family}_fact_{}",
                    if required { "missing" } else { "unrequested" }
                ),
            ));
        }
    }
    Ok(())
}

fn validate_multiline_fact(field: &'static str, value: &str) -> Result<(), ContractError> {
    if value.is_empty() || value.len() > 65_536 || value.contains('\0') || value.contains('\r') {
        return Err(ContractError::identity(field, "invalid_fact"));
    }
    Ok(())
}

fn validate_sha256_hex(value: &str) -> Result<(), ContractError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(ContractError::identity(
            "qualification.runtime_identity.tool_sha256",
            "invalid_digest",
        ))
    }
}

fn validate_image_identity(
    requirements: &QualificationRuntimeIdentityRequirements,
    platform: &QualificationRuntimePlatform,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;
    use crate::config::RunsOn;

    match (RunsOn::parse(&platform.runs_on)?, &platform.image_identity) {
        (
            RunsOn::Hosted(label),
            QualificationRuntimeImageIdentity::Hosted {
                image_os,
                image_version,
            },
        ) if label == platform.runner_label => {
            for (field, value) in [
                ("image_os", image_os.as_str()),
                ("image_version", image_version.as_str()),
            ] {
                validate_semantic_text(field, value)?;
                if value == crate::UNOBSERVED_IMAGE_VALUE {
                    return Err(ContractError::identity(
                        "qualification.runtime_identity",
                        format!("unobserved:{field}"),
                    ));
                }
            }
        }
        (
            RunsOn::ScaleSet(selector),
            QualificationRuntimeImageIdentity::ScaleSet {
                source_sha,
                guest_platform,
                scale_set,
                runner_image,
                dind_image,
            },
        ) => {
            if selector.token() != platform.runs_on
                || selector.name() != scale_set.name
                || selector.labels() != scale_set.labels
                || guest_platform != "linux/amd64"
                || platform.os != "Linux"
                || platform.arch != "X64"
                || platform.target != "x86_64-unknown-linux-gnu"
                || source_sha.len() != 40
                || !source_sha
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                || requirements.scale_set_image_source_sha.as_deref() != Some(source_sha)
            {
                return Err(ContractError::identity(
                    "qualification.runtime_identity.image_identity",
                    if requirements.scale_set_image_source_sha.is_none() {
                        "missing_expected_scale_set_image_source"
                    } else {
                        "scale_set_image_mismatch"
                    },
                ));
            }
            for label in &scale_set.labels {
                validate_semantic_text("scale_set_label", label)?;
            }
            validate_semantic_text("scale_set_name", &scale_set.name)?;
            validate_image_digests(runner_image)?;
            validate_image_digests(dind_image)?;
        }
        _ => {
            return Err(ContractError::identity(
                "qualification.runtime_identity.image_identity",
                "image_source_mismatch",
            ));
        }
    }
    Ok(())
}

fn validate_image_digests(image: &QualificationRuntimeImageDigests) -> Result<(), ContractError> {
    let valid_hex = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    if !valid_hex(&image.archive_sha256)
        || !valid_hex(&image.archive_index_digest)
        || image
            .image_index_digest
            .as_deref()
            .is_some_and(|digest| !valid_oci_digest(digest))
        || !valid_oci_digest(&image.manifest_digest)
        || !valid_oci_digest(&image.config_digest)
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.image_identity",
            "malformed_scale_set_image_digest",
        ));
    }
    Ok(())
}

fn valid_oci_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
