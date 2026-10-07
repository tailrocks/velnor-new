//! Strict scenario evidence bound to the source plan and named check.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use velnor_actions_contract::{digest_b3, parse_strict_json};
use velnor_actions_contract_config::config::{CheckEvidence, CheckPlatform};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;
use velnor_actions_orchestrator_core::link_safety::reject_link_components;

/// Producer payload; unknown fields and duplicate JSON keys fail closed.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioEvidence {
    pub schema: u32,
    pub source: String,
    pub head: String,
    pub check_id: String,
    pub platform: CheckPlatform,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub executed: bool,
    pub status: ScenarioStatus,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioStatus {
    Passed,
    Failed,
    Skipped,
}

/// Trusted receipt records precisely the bytes examined by this helper.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReceipt {
    pub schema: u32,
    pub check_id: String,
    pub source: String,
    pub head: String,
    pub platform: CheckPlatform,
    pub path: String,
    pub digest: String,
    pub scenarios: Vec<String>,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

/// Validate declared scenarios and produce a byte-bound receipt.
pub fn verify_evidence(
    root: &Path,
    declaration: &CheckEvidence,
    id: &str,
    head: &str,
    platform: CheckPlatform,
) -> Result<EvidenceReceipt, OrchestratorError> {
    reject_link_components(root, &declaration.path)?;
    let text = match velnor_actions_orchestrator_core::safe_read::read_repo_file(
        root,
        &declaration.path,
        velnor_actions_orchestrator_core::safe_read::MAX_REPO_FILE_BYTES,
    )? {
        velnor_actions_orchestrator_core::safe_read::RepoRead::Absent => {
            return Err(internal("check_evidence_missing"));
        }
        velnor_actions_orchestrator_core::safe_read::RepoRead::Text(text) if !text.is_empty() => {
            text
        }
        velnor_actions_orchestrator_core::safe_read::RepoRead::Text(_) => {
            return Err(internal("check_evidence_empty"));
        }
    };
    let value = parse_strict_json(&text).map_err(|_| internal("check_evidence_json"))?;
    let evidence: ScenarioEvidence =
        serde_json::from_value(value).map_err(|_| internal("check_evidence_shape"))?;
    validate_evidence(&evidence, declaration, id, head, platform)?;
    Ok(EvidenceReceipt {
        schema: 1,
        check_id: id.to_owned(),
        source: "mise-task-v1".to_owned(),
        head: head.to_owned(),
        platform,
        path: declaration.path.clone(),
        digest: digest_b3(text.as_bytes()),
        scenarios: declaration.expected_scenarios.clone(),
        bytes: text.into_bytes(),
    })
}

pub fn validate_evidence(
    evidence: &ScenarioEvidence,
    declaration: &CheckEvidence,
    id: &str,
    head: &str,
    platform: CheckPlatform,
) -> Result<(), OrchestratorError> {
    if evidence.schema != 1
        || evidence.source != "mise-task-v1"
        || evidence.head != head
        || evidence.check_id != id
        || evidence.platform != platform
    {
        return Err(internal("check_evidence_identity"));
    }
    let expected: BTreeSet<_> = declaration
        .expected_scenarios
        .iter()
        .map(String::as_str)
        .collect();
    if expected.is_empty() || expected.len() != declaration.expected_scenarios.len() {
        return Err(internal("check_evidence_expected_scenarios"));
    }
    let mut actual = BTreeSet::new();
    for scenario in &evidence.scenarios {
        if !actual.insert(scenario.id.as_str()) || !expected.contains(scenario.id.as_str()) {
            return Err(internal("check_evidence_scenario_identity"));
        }
        if !scenario.executed || scenario.status != ScenarioStatus::Passed {
            return Err(internal("check_evidence_scenario_not_passed"));
        }
    }
    if actual != expected {
        return Err(internal("check_evidence_scenarios_missing"));
    }
    Ok(())
}
