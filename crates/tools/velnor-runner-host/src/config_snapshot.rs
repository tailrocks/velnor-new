//! One-read validated host configuration and its exact source digest.

use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_runner_docker_spec::{RunnerImageProfile, resolve_linux_admission_profile};
use velnor_runner_github::policy::{
    JobTrustPolicyView, JobTrustRuleView, ReusableWorkflowRuleView,
};
use velnor_runner_host_config::{
    HostConfig, HostPlatform, ScaleSetBinding, read_host_config_bytes,
};

use crate::HostError;

/// Immutable validated view of one protected configuration file.
///
/// The digest is SHA-256 over the exact byte buffer parsed and validated by
/// this loader. Fields are private so callers cannot pair a digest with a
/// different configuration or binding.
#[derive(Debug)]
pub struct ValidatedHostConfigSnapshot {
    config: HostConfig,
    policy_digest: String,
    binding: ScaleSetBinding,
    runner_image_profile: Option<RunnerImageProfile>,
    platform: HostPlatform,
}

impl ValidatedHostConfigSnapshot {
    /// Parsed, validated configuration retained from the protected bytes.
    #[must_use]
    pub const fn config(&self) -> &HostConfig {
        &self.config
    }

    /// Lowercase SHA-256 of the exact protected bytes parsed for this snapshot.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    /// Exact typed registration scope and target/group/set binding.
    #[must_use]
    pub const fn scale_set_binding(&self) -> &ScaleSetBinding {
        &self.binding
    }

    /// Resolved immutable image profile, absent when no approved profile can be
    /// resolved. In particular, Ubuntu 26 config remains usable for cleanup and
    /// reconciliation while runner admission/start stays unavailable.
    #[must_use]
    pub const fn runner_image_profile(&self) -> Option<RunnerImageProfile> {
        self.runner_image_profile
    }

    /// Host backend validated for this snapshot.
    #[must_use]
    pub const fn platform(&self) -> HostPlatform {
        self.platform
    }

    /// Build the exact borrowed per-offer trust view from this validated
    /// snapshot and invoke `use_view` while all backing strings remain owned
    /// by the snapshot. Missing new rule fields return `None`; no event,
    /// branch, workflow path, ref, or reusable chain is inferred or expanded
    /// from the coarse lists.
    #[must_use]
    pub fn with_job_trust_policy_view<R>(
        &self,
        use_view: impl FnOnce(JobTrustPolicyView<'_>) -> R,
    ) -> Option<R> {
        let policy = self.config.trust.as_ref()?;
        if policy.allowed_head_branches.is_empty() || policy.workflow_rules.is_empty() {
            return None;
        }

        let reusable_workflows: Vec<Vec<ReusableWorkflowRuleView<'_>>> = policy
            .workflow_rules
            .iter()
            .map(|rule| {
                rule.referenced_workflows
                    .iter()
                    .map(|workflow| ReusableWorkflowRuleView {
                        path: &workflow.path,
                        git_ref: &workflow.git_ref,
                        sha: &workflow.sha,
                    })
                    .collect()
            })
            .collect();
        let workflow_rules: Vec<JobTrustRuleView<'_>> = policy
            .workflow_rules
            .iter()
            .zip(&reusable_workflows)
            .map(|(rule, referenced_workflows)| JobTrustRuleView {
                workflow_ref: &rule.workflow_ref,
                job_workflow_ref: &rule.job_workflow_ref,
                workflow_path: &rule.workflow_path,
                event: &rule.event,
                head_branch: &rule.head_branch,
                referenced_workflows,
            })
            .collect();
        Some(use_view(JobTrustPolicyView {
            repository_full_name: &self.config.github.repository,
            allowed_repositories: &policy.allowed_repositories,
            allowed_events: &policy.allowed_events,
            allowed_workflow_paths: &policy.allowed_workflow_paths,
            allowed_head_branches: &policy.allowed_head_branches,
            workflow_rules: &workflow_rules,
            allow_forks: policy.allow_forks,
            policy_digest: &self.policy_digest,
        }))
    }
}

/// Read and validate one protected configuration snapshot.
///
/// The protected loader returns exact bytes once. This function hashes and
/// parses that same buffer, then derives the typed binding and immutable image
/// profile without reopening the path.
///
/// Missing legacy macOS configuration is `Ok(None)`. A missing Linux system
/// configuration is an error because the Linux backend requires an installed
/// package-owned file.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the file is missing on Linux, malformed,
/// unsafe, for another platform, or has an invalid binding/profile.
pub fn read_validated_host_config_snapshot(
    path: &Path,
    platform: HostPlatform,
) -> Result<Option<ValidatedHostConfigSnapshot>, HostError> {
    snapshot_from_optional_bytes(read_host_config_bytes(path, platform)?, platform)
}

fn snapshot_from_optional_bytes(
    bytes: Option<Vec<u8>>,
    platform: HostPlatform,
) -> Result<Option<ValidatedHostConfigSnapshot>, HostError> {
    let Some(bytes) = bytes else {
        return if platform == HostPlatform::Linux {
            Err(HostError::Config)
        } else {
            Ok(None)
        };
    };
    snapshot_from_bytes(&bytes, platform).map(Some)
}

fn snapshot_from_bytes(
    bytes: &[u8],
    platform: HostPlatform,
) -> Result<ValidatedHostConfigSnapshot, HostError> {
    let policy_digest = lower_hex(&Sha256::digest(bytes));
    let text = std::str::from_utf8(bytes).map_err(|_| HostError::Config)?;
    let config = HostConfig::parse(text)?;
    config.validate_for_host(platform)?;
    let binding = config.scale_set_binding()?;
    // Only a source-pinned, evidence-reviewed Ubuntu 26 profile can cross the
    // admission boundary. Current builds intentionally have no such pin, so a
    // valid cleanup configuration retains its binding but no runnable image.
    let runner_image_profile = if platform == HostPlatform::Linux {
        binding
            .runner_image_profile
            .as_deref()
            .and_then(|key| resolve_linux_admission_profile(key, &binding.scale_set_name).ok())
    } else {
        None
    };
    Ok(ValidatedHostConfigSnapshot {
        config,
        policy_digest,
        binding,
        runner_image_profile,
        platform,
    })
}

fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests;
