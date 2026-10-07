use super::super::{HostConfig, JobTrustPolicy};
use super::LINUX;
use crate::HostError;

#[test]
fn trust_policy_rejects_wrong_event_repository_and_unproved_fork() -> Result<(), HostError> {
    let policy = HostConfig::parse(LINUX)?.job_trust_policy()?;
    assert!(!policy.allows(
        "ChainArgos",
        "java-monorepo",
        "workflow_dispatch",
        Some("ChainArgos/java-monorepo"),
        ".github/workflows/ci.yml",
    ));
    assert!(!policy.allows(
        "attacker",
        "java-monorepo",
        "push",
        Some("attacker/java-monorepo"),
        ".github/workflows/ci.yml",
    ));
    assert!(!policy.allows(
        "ChainArgos",
        "java-monorepo",
        "pull_request",
        None,
        ".github/workflows/ci.yml",
    ));
    assert!(!policy.allows(
        "ChainArgos",
        "java-monorepo",
        "pull_request",
        Some("fork-user/java-monorepo"),
        ".github/workflows/ci.yml",
    ));
    assert!(!policy.allows(
        "ChainArgos",
        "java-monorepo",
        "pull_request",
        Some("ChainArgos/java-monorepo"),
        ".github/workflows/other.yml",
    ));
    Ok(())
}

#[test]
fn trust_policy_rejects_duplicate_and_wildcard_entries() {
    for text in [
        LINUX.replace(
            "allowed_events = [\"push\", \"pull_request\"]",
            "allowed_events = [\"push\", \"push\"]",
        ),
        LINUX.replace(
            "allowed_repositories = [\"ChainArgos/java-monorepo\"]",
            "allowed_repositories = [\"*\"]",
        ),
        LINUX.replace(
            "allowed_events = [\"push\", \"pull_request\"]",
            "allowed_events = [\"*\"]",
        ),
        LINUX.replace(
            "allowed_workflow_paths = [\".github/workflows/ci.yml\"]",
            "allowed_workflow_paths = [\"*\"]",
        ),
        LINUX.replace(
            "allowed_workflow_paths = [\".github/workflows/ci.yml\"]",
            "allowed_workflow_paths = [\".github/workflows/../ci.yml\"]",
        ),
        LINUX.replace(
            "allowed_workflow_paths = [\".github/workflows/ci.yml\"]",
            "allowed_workflow_paths = [\".github/workflows/ci.yml\", \".github/workflows/ci.yml\"]",
        ),
    ] {
        assert!(HostConfig::parse(&text).is_err());
    }
}

#[test]
fn runner_group_name_rejects_control_characters() {
    let text = LINUX.replace(
        "runner_group_name = \"Default\"",
        "runner_group_name = \"bad\\nname\"",
    );
    assert!(HostConfig::parse(&text).is_err());
}

#[test]
fn trust_struct_remains_secret_free() {
    let policy = JobTrustPolicy {
        allowed_repositories: vec!["ChainArgos/java-monorepo".to_owned()],
        allowed_events: vec!["push".to_owned()],
        allowed_workflow_paths: vec![".github/workflows/ci.yml".to_owned()],
        allow_forks: false,
    };
    assert!(!format!("{policy:?}").contains("token"));
}
