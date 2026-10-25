use super::DiscoveredCheck;
use serde::ser::SerializeMap;

#[derive(Debug, serde::Serialize)]
pub struct CheckEntryMetadata<'a> {
    pub check_id: &'a str,
    pub task: &'a str,
    pub directory: &'a str,
    pub runner: &'a velnor_actions_contract::config::CheckRunner,
    pub tools: &'a [String],
    pub qualified_tools: &'a [velnor_actions_contract::config::QualifiedTool],
    pub qualification_digest: &'a str,
    pub tool_specs: &'a [String],
    pub system_tools: &'a [velnor_actions_contract::config::CheckSystemTool],
    pub evidence: &'a Option<velnor_actions_contract::config::CheckEvidence>,
    pub timeout_minutes: u32,
    #[serde(flatten)]
    policy: OpaqueExecutionPolicy,
}

impl DiscoveredCheck {
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
