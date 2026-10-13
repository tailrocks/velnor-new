use super::{HostConfig, HostPlatform, JobTrustPolicy};
use crate::HostError;

const LINUX: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"ChainArgos/java-monorepo\"\n",
    "scale_set_name = \"ubuntu-24.04-scale-set\"\n",
    "credential_ref = \"systemd-credential:github-token\"\n",
    "registration_scope = \"repository\"\n",
    "runner_group_id = 1\n",
    "runner_group_name = \"Default\"\n",
    "[host]\n",
    "platform = \"linux\"\n",
    "max_jobs = 1\n",
    "drain_timeout_secs = 1800\n",
    "[trust]\n",
    "allowed_repositories = [\"ChainArgos/java-monorepo\"]\n",
    "allowed_events = [\"push\", \"pull_request\"]\n",
    "allowed_workflow_paths = [\".github/workflows/ci.yml\"]\n",
    "allow_forks = false\n",
    "[runner]\n",
    "image_profile = \"ubuntu-24.04-amd64\"\n",
    "[docker]\n",
    "context = \"system\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

#[test]
fn linux_config_binds_explicit_scope_group_trust_profile_and_drain() -> Result<(), HostError> {
    let config = HostConfig::parse(LINUX)?;
    config.validate_for_host(HostPlatform::Linux)?;
    assert_eq!(config.drain_timeout_secs()?, 1800);
    let binding = config.scale_set_binding()?;
    assert_eq!(binding.owner, "ChainArgos");
    assert_eq!(binding.repository, "java-monorepo");
    assert_eq!(binding.runner_group_id, 1);
    assert_eq!(binding.runner_group_name, "Default");
    assert_eq!(
        binding.runner_image_profile.as_deref(),
        Some("ubuntu-24.04-amd64")
    );
    assert!(config.job_trust_policy()?.allows(
        "ChainArgos",
        "java-monorepo",
        "pull_request",
        Some("ChainArgos/java-monorepo"),
        ".github/workflows/ci.yml",
    ));
    Ok(())
}

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
fn linux_requires_explicit_matching_trust_scope_credentials_and_image() {
    let invalid = [
        LINUX.replace("max_jobs = 1\n", ""),
        LINUX.replace("registration_scope = \"repository\"\n", ""),
        LINUX.replace("runner_group_name = \"Default\"\n", ""),
        LINUX.replace("allow_forks = false", "allow_forks = true"),
        LINUX.replace(
            "allowed_workflow_paths = [\".github/workflows/ci.yml\"]\n",
            "",
        ),
        LINUX.replace(
            "systemd-credential:github-token",
            "keychain:com.example/token",
        ),
        LINUX.replace("platform = \"linux\"", "platform = \"macos\""),
        LINUX.replace("ubuntu-24.04-scale-set", "ubuntu-26.04-scale-set"),
    ];
    for text in invalid {
        let accepted = HostConfig::parse(&text)
            .and_then(|config| config.validate_for_host(HostPlatform::Linux))
            .is_ok();
        assert!(!accepted, "invalid Linux config was accepted");
    }
}

#[test]
fn linux_profile_selector_accepts_dotted_supported_version() -> Result<(), HostError> {
    let config = HostConfig::parse(LINUX)?;
    config.validate_for_host(HostPlatform::Linux)?;
    assert_eq!(config.github.scale_set_name, "ubuntu-24.04-scale-set");
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
fn legacy_mac_config_can_be_parsed_without_selecting_linux() -> Result<(), HostError> {
    let config = HostConfig::parse(concat!(
        "schema = 1\n",
        "[github]\n",
        "repository = \"tailrocks/velnor-new\"\n",
        "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
        "credential_ref = \"keychain:com.tailrocks.velnor.host/velnor-host\"\n",
        "[host]\n",
        "[docker]\n",
        "context = \"orbstack\"\n",
        "platform = \"linux/amd64\"\n",
        "endpoint = \"unix:///var/run/docker.sock\"\n",
    ))?;
    assert_eq!(config.host.platform, None);
    assert_eq!(config.max_jobs(), 1);
    config.validate_for_host(HostPlatform::Macos)?;
    assert_eq!(config.scale_set_binding()?.runner_group_id, 1);
    assert_eq!(config.scale_set_binding()?.runner_group_name, "Default");
    assert!(!config.is_connect_managed());
    Ok(())
}

#[test]
fn connect_ownership_marker_is_optional_but_has_one_supported_value() -> Result<(), HostError> {
    let managed = HostConfig::parse(&LINUX.replace(
        "schema = 1\n",
        "schema = 1\nmanaged_by = \"velnor-host-connect-v1\"\n",
    ))?;
    assert!(managed.is_connect_managed());
    assert!(
        HostConfig::parse(
            &LINUX.replace("schema = 1\n", "schema = 1\nmanaged_by = \"operator\"\n")
        )
        .is_err()
    );
    let legacy = HostConfig::parse(LINUX)?;
    assert!(!legacy.is_connect_managed());
    Ok(())
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
