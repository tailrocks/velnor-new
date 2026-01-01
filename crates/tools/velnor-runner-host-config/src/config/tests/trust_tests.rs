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
fn exact_group_workflow_selectors_are_separate_and_repository_bound() -> Result<(), HostError> {
    let valid = LINUX.replace(
        "allowed_workflow_paths = [\".github/workflows/ci.yml\"]",
        "allowed_workflow_paths = [\".github/workflows/ci.yml\"]\nallowed_group_workflows = [\"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\"]",
    );
    let parsed = HostConfig::parse(&valid)?;
    assert_eq!(
        parsed.job_trust_policy()?.allowed_group_workflows,
        ["ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main"]
    );
    let separate_group_selector = valid.replace(
        "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
        "ChainArgos/java-monorepo/.github/workflows/reusable.yml@refs/heads/release",
    );
    assert!(HostConfig::parse(&separate_group_selector).is_ok());
    let yaml_group_selector = valid.replace(
        "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
        "ChainArgos/java-monorepo/.github/workflows/ci.yaml@refs/heads/main",
    );
    assert!(HostConfig::parse(&yaml_group_selector).is_ok());

    let invalid = [
        valid.replace(
            "allowed_group_workflows = [\"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\"]",
            "allowed_group_workflows = [\"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\", \"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\"]",
        ),
        valid.replace(
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
            "other/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
        ),
        valid.replace(
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
            "ChainArgos/java-monorepo/.github/workflows/ci.yml",
        ),
        valid.replace(
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
            "ChainArgos/java-monorepo/.github/workflows/../ci.yml@refs/heads/main",
        ),
        valid.replace(
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/*",
        ),
        valid.replace(
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
            "ChainArgos/java-monorepo/.github/workflows/.yml@refs/heads/main",
        ),
        valid.replace(
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main@other",
        ),
        valid.replace(
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main",
            "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main]",
        ),
    ];
    for text in invalid {
        assert!(HostConfig::parse(&text).is_err());
    }
    Ok(())
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
        allowed_head_branches: Vec::new(),
        workflow_rules: Vec::new(),
        allowed_group_workflows: Vec::new(),
        allow_forks: false,
    };
    assert!(!format!("{policy:?}").contains("token"));
}

const STRUCTURED_RULES: &str = concat!(
    "allowed_head_branches = [\"main\", \"release\"]\n",
    "allowed_group_workflows = [\"ChainArgos/java-monorepo/.github/workflows/ci.yml@main\"]\n",
    "allow_forks = false\n",
    "[[trust.workflow_rules]]\n",
    "workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/ci.yml@main\"\n",
    "job_workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/reuse.yml@refs/tags/v1\"\n",
    "workflow_path = \".github/workflows/ci.yml@main\"\n",
    "event = \"push\"\n",
    "head_branch = \"main\"\n",
    "[[trust.workflow_rules.referenced_workflows]]\n",
    "path = \"ChainArgos/java-monorepo/.github/workflows/reuse.yml@v1\"\n",
    "git_ref = \"refs/tags/v1\"\n",
    "sha = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    "[[trust.workflow_rules]]\n",
    "workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/ci.yml@main\"\n",
    "job_workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\"\n",
    "workflow_path = \".github/workflows/ci.yml@main\"\n",
    "event = \"pull_request\"\n",
    "head_branch = \"release\"\n",
);

fn config_with_rules() -> String {
    super::LINUX.replace("allow_forks = false\n", STRUCTURED_RULES)
}

#[test]
fn structured_workflow_rules_are_preserved_as_exact_tuples() -> Result<(), HostError> {
    let config = HostConfig::parse(&config_with_rules())?;
    let policy = config.job_trust_policy()?;
    assert_eq!(policy.allowed_head_branches, ["main", "release"]);
    assert_eq!(policy.workflow_rules.len(), 2);
    assert_eq!(
        policy.workflow_rules[0].workflow_ref,
        "ChainArgos/java-monorepo/.github/workflows/ci.yml@main"
    );
    assert_eq!(policy.workflow_rules[0].event, "push");
    assert_eq!(policy.workflow_rules[0].head_branch, "main");
    assert_eq!(
        policy.workflow_rules[0].referenced_workflows[0].path,
        "ChainArgos/java-monorepo/.github/workflows/reuse.yml@v1"
    );
    assert_eq!(
        policy.workflow_rules[0].referenced_workflows[0].git_ref,
        "refs/tags/v1"
    );
    assert_eq!(policy.workflow_rules[1].event, "pull_request");
    assert_eq!(policy.workflow_rules[1].head_branch, "release");
    Ok(())
}

#[test]
fn structured_rules_reject_mismatched_or_unsafe_tuple_values() {
    let config = config_with_rules();
    for invalid in [
        config.replace(
            "workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/ci.yml@main\"",
            "workflow_ref = \"attacker/repo/.github/workflows/ci.yml@main\"",
        ),
        config.replace("head_branch = \"main\"", "head_branch = \"other\""),
        config.replace(
            "workflow_path = \".github/workflows/ci.yml@main\"",
            "workflow_path = \".github/workflows/other.yml@main\"",
        ),
        config.replace(
            "sha = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
            "sha = \"not-a-commit\"",
        ),
    ] {
        assert!(HostConfig::parse(&invalid).is_err());
    }
}
