use super::super::{HostConfig, HostPlatform, RegistrationScope};
use super::LINUX;
use crate::HostError;

#[test]
fn linux_config_binds_explicit_scope_group_trust_profile_and_drain() -> Result<(), HostError> {
    let config = HostConfig::parse(LINUX)?;
    config.validate_for_host(HostPlatform::Linux)?;
    assert_eq!(config.drain_timeout_secs()?, 1800);
    let binding = config.scale_set_binding()?;
    assert_eq!(
        binding.scope,
        RegistrationScope::Repository {
            owner: "ChainArgos".to_owned(),
            repository: "java-monorepo".to_owned(),
        }
    );
    assert_eq!(binding.owner, "ChainArgos");
    assert_eq!(binding.repository, "java-monorepo");
    assert_eq!(binding.runner_group_id, 1);
    assert_eq!(binding.runner_group_name, "Default");
    assert_eq!(
        binding.runner_image_profile.as_deref(),
        Some("ubuntu-26.04-amd64")
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
fn organization_scope_is_typed_and_separate_from_the_trust_target() -> Result<(), HostError> {
    let text = LINUX.replace(
        "registration_scope = \"repository\"\n",
        "registration_scope = \"organization\"\nregistration_scope_name = \"ChainArgos\"\n",
    );
    let config = HostConfig::parse(&text)?;
    config.validate_for_host(HostPlatform::Linux)?;
    let binding = config.scale_set_binding()?;
    assert_eq!(
        binding.scope,
        RegistrationScope::Organization {
            organization: "ChainArgos".to_owned(),
        }
    );
    assert_eq!(binding.owner, "ChainArgos");
    assert_eq!(binding.repository, "java-monorepo");
    assert_eq!(
        config.job_trust_policy()?.allowed_repositories,
        ["ChainArgos/java-monorepo"]
    );
    Ok(())
}

#[test]
fn organization_scope_rejects_missing_mismatched_and_mac_values() {
    let missing_name = LINUX.replace(
        "registration_scope = \"repository\"\n",
        "registration_scope = \"organization\"\n",
    );
    assert!(HostConfig::parse(&missing_name).is_err());

    let mismatch = LINUX.replace(
        "registration_scope = \"repository\"\n",
        "registration_scope = \"organization\"\nregistration_scope_name = \"other-org\"\n",
    );
    assert!(HostConfig::parse(&mismatch).is_err());

    let repository_with_name = LINUX.replace(
        "registration_scope = \"repository\"\n",
        "registration_scope = \"repository\"\nregistration_scope_name = \"ChainArgos\"\n",
    );
    assert!(HostConfig::parse(&repository_with_name).is_err());

    let mac = HostConfig::parse(&LINUX.replace(
        "registration_scope = \"repository\"\n",
        "registration_scope = \"organization\"\nregistration_scope_name = \"ChainArgos\"\n",
    ))
    .expect("organization syntax is valid");
    assert!(mac.validate_for_host(HostPlatform::Macos).is_err());
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
        LINUX.replace("ubuntu-26.04-scale-set", "ubuntu-24.04-scale-set"),
    ];
    for text in invalid {
        let accepted = HostConfig::parse(&text)
            .and_then(|config| config.validate_for_host(HostPlatform::Linux))
            .is_ok();
        assert!(!accepted, "invalid Linux config was accepted");
    }
}

#[test]
fn linux_profile_selector_requires_ubuntu_26() -> Result<(), HostError> {
    let config = HostConfig::parse(LINUX)?;
    config.validate_for_host(HostPlatform::Linux)?;
    assert_eq!(config.github.scale_set_name, "ubuntu-26.04-scale-set");
    assert!(
        HostConfig::parse(&LINUX.replace("ubuntu-26.04-amd64", "ubuntu-24.04-amd64",))
            .and_then(|config| config.validate_for_host(HostPlatform::Linux))
            .is_err()
    );
    Ok(())
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
