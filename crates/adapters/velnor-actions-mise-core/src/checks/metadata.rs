//! Adapter metadata with one sealed opaque execution policy.
use super::DiscoveredCheck;
use serde::ser::SerializeMap;

/// Adapter-owned metadata; orchestrators serialize this projection verbatim.
#[derive(Debug, serde::Serialize)]
pub struct CheckEntryMetadata<'a> {
    /// Stable configured obligation.
    pub check_id: &'a str,
    /// Native Mise task name.
    pub task: &'a str,
    /// Repository-relative working directory.
    pub directory: &'a str,
    /// Explicit typed placement and complete host container profile.
    pub runner: &'a velnor_actions_contract::config::CheckRunner,
    /// Explicit qualified tool IDs.
    pub tools: &'a [String],
    /// Full explicit qualified installation closure; adapter-owned transport.
    pub qualified_tools: &'a [velnor_actions_contract::config::QualifiedTool],
    /// Canonical declarations, options, dependencies and source qualification digest.
    pub qualification_digest: &'a str,
    /// Exact selected backend selectors participating in the fingerprint.
    pub tool_specs: &'a [String],
    /// Exact observed native system-tool versions and builds.
    pub system_tools: &'a [velnor_actions_contract::config::CheckSystemTool],
    /// Required machine-readable evidence contract.
    pub evidence: &'a Option<velnor_actions_contract::config::CheckEvidence>,
    /// Process deadline in minutes.
    pub timeout_minutes: u32,
    #[serde(flatten)]
    policy: OpaqueExecutionPolicy,
}

impl DiscoveredCheck {
    /// Stable metadata contract owned by this adapter.
    #[must_use]
    pub fn entry_metadata(&self) -> CheckEntryMetadata<'_> {
        CheckEntryMetadata {
            check_id: &self.check.id,
            task: &self.check.task,
            directory: &self.check.directory,
            runner: &self.check.runner,
            tools: &self.check.tools,
            qualified_tools: &self.qualified_tools,
            qualification_digest: &self.qualification_digest,
            tool_specs: &self.tool_specs,
            system_tools: &self.check.system_tools,
            evidence: &self.check.evidence,
            timeout_minutes: self.check.timeout_minutes,
            policy: OpaqueExecutionPolicy,
        }
    }
}

/// This adapter supports exactly one policy: opaque execution without reuse.
#[derive(Debug)]
struct OpaqueExecutionPolicy;

impl serde::Serialize for OpaqueExecutionPolicy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("opaque", &true)?;
        map.serialize_entry("allow_compilation_reuse", &false)?;
        map.serialize_entry("allow_task_reuse", &false)?;
        map.serialize_entry("task_cache_enabled", &false)?;
        map.serialize_entry("artifact_cache_enabled", &false)?;
        map.end()
    }
}
