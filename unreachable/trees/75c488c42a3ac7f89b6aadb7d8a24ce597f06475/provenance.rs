//! Per-workspace profile provenance recorded in the generate report.

use crate::prepare::GenerationPreparation;

/// One evidence sighting recorded in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceProvenance {
    /// Repository-relative evidence path.
    pub path: String,
    /// One-based line number.
    pub line: u32,
    /// Matched setting or command text.
    pub command_or_setting: String,
    /// Durability (`durable` or `transient`).
    pub strength: String,
}

/// Per-workspace profile provenance recorded in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProvenance {
    /// Workspace root; empty for the repository root.
    pub workspace_root: String,
    /// Selected compile driver.
    pub compile_driver: String,
    /// Driver provenance (`declared` or `detected`).
    pub driver_source: String,
    /// Selected test runner.
    pub test_runner: String,
    /// Runner provenance (`declared` or `detected`).
    pub runner_source: String,
    /// Selected Nextest profile (`ci` or `default`).
    pub nextest_profile: String,
    /// Nearest consumed `.config/nextest.toml`, when any exists.
    pub nextest_config: Option<String>,
    /// Evidence backing the selection, sorted.
    pub evidence: Vec<EvidenceProvenance>,
}

/// Per-workspace profile provenance for the report.
pub(crate) fn profile_provenance(prep: &GenerationPreparation) -> Vec<ProfileProvenance> {
    prep.discovery
        .workspaces
        .iter()
        .map(|workspace| {
            let profile = &workspace.profile;
            ProfileProvenance {
                workspace_root: workspace.record.workspace_root.clone(),
                compile_driver: profile.compile_driver.as_str().to_owned(),
                driver_source: profile.driver_source.as_str().to_owned(),
                test_runner: profile.test_runner.as_str().to_owned(),
                runner_source: profile.runner_source.as_str().to_owned(),
                nextest_profile: profile.nextest_profile.as_str().to_owned(),
                nextest_config: profile.nextest_config.clone(),
                evidence: profile
                    .evidence
                    .iter()
                    .map(|sighting| EvidenceProvenance {
                        path: sighting.path.clone(),
                        line: sighting.line,
                        command_or_setting: sighting.command_or_setting.clone(),
                        strength: sighting.strength.as_str().to_owned(),
                    })
                    .collect(),
            }
        })
        .collect()
}
