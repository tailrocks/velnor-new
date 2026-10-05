//! MBX 1.22 stays a held observation; generated qualification remains protected-main only.

use serde_json::Value;
use velnor_actions_mise::{
    MR_BOXINGTON_PR_QUALIFICATION_SHA, MR_BOXINGTON_PR_QUALIFICATION_VERSION,
};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{required_file, workflow_config};

const ACTION_CANDIDATE_SHA: &str = "d0825fbaf3cc36ca2609aa38e71046265a1f1e37";

#[test]
fn generated_qualification_has_no_unqualified_pull_request_cache_writer() -> TestResult {
    let repo = make_repo(&workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = required_file(&tree, ".github/workflows/qualification.yml")?;

    assert!(!workflow.contains("pull_request:"), "{workflow}");
    assert!(!workflow.contains("mbx-pr-candidate-write"), "{workflow}");
    assert!(!workflow.contains("mbx-pr-candidate-read"), "{workflow}");
    Ok(())
}

#[test]
fn latest_mbx_observation_does_not_promote_production_pins() -> TestResult {
    let inventory: Value =
        serde_json::from_str(include_str!("../../../.velnor/freshness-inventory.json"))?;
    let version_policy: Value =
        toml::from_str(include_str!("../../../.velnor/version-policy.toml"))?;
    assert_eq!(
        version_policy["tools"]["mr-boxington"].as_str(),
        Some("1.21.1")
    );
    let tool = inventory["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|item| item["name"] == "mr-boxington"))
        .ok_or("missing MBX freshness entry")?;
    assert_eq!(
        tool["latest"],
        format!("v{MR_BOXINGTON_PR_QUALIFICATION_VERSION}")
    );
    assert_eq!(tool["latest_sha"], MR_BOXINGTON_PR_QUALIFICATION_SHA);
    assert_eq!(tool["pinned"], "1.21.1");
    assert_eq!(tool["qualified"], "1.21.1");
    assert_eq!(tool["status"], "held");

    let action = inventory["actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|item| item["key"] == "jdx/mr-boxington-action")
        })
        .ok_or("missing MBX action freshness entry")?;
    assert_eq!(action["latest"], "v1.7.1");
    assert_eq!(action["latest_sha"], ACTION_CANDIDATE_SHA);
    assert_eq!(action["pinned_version"], "v1.6.0");
    assert_eq!(
        action["pinned_sha"],
        "1687e54eb349cadf61fa38b5813a77875489e8e6"
    );
    Ok(())
}
