//! Schema boundary: explicit Linux bindings validate, partial or
//! foreign configs fail closed, legacy macOS parses without Linux.

use velnor_runner_host_config::{HostConfig, HostPlatform};

const LINUX: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"ChainArgos/java-monorepo\"\n",
    "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
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
    "image_profile = \"ubuntu-26.04-amd64\"\n",
    "[docker]\n",
    "context = \"system\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

#[test]
fn linux_binding_contract_holds() -> Result<(), String> {
    let config = HostConfig::parse(LINUX).map_err(|error| error.to_string())?;
    config
        .validate_for_host(HostPlatform::Linux)
        .map_err(|error| error.to_string())?;
    let binding = config
        .scale_set_binding()
        .map_err(|error| error.to_string())?;
    assert_eq!(binding.owner, "ChainArgos");
    assert_eq!(binding.runner_group_id, 1);
    assert!(
        config
            .job_trust_policy()
            .map_err(|error| error.to_string())?
            .allows(
                "ChainArgos",
                "java-monorepo",
                "push",
                Some("ChainArgos/java-monorepo"),
                ".github/workflows/ci.yml",
            )
    );
    Ok(())
}

#[test]
fn partial_and_foreign_configs_fail_closed() {
    for text in [
        LINUX.replace("registration_scope = \"repository\"\n", ""),
        LINUX.replace("allow_forks = false", "allow_forks = true"),
        LINUX.replace("platform = \"linux\"", "platform = \"macos\""),
        "schema = 2\n".to_owned(),
        String::new(),
    ] {
        let accepted = HostConfig::parse(&text)
            .and_then(|config| config.validate_for_host(HostPlatform::Linux))
            .is_ok();
        assert!(!accepted, "invalid Linux config was accepted");
    }
}

#[test]
fn legacy_mac_parses_without_selecting_linux() -> Result<(), String> {
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
    ))
    .map_err(|error| error.to_string())?;
    assert_eq!(config.host.platform, None);
    config
        .validate_for_host(HostPlatform::Macos)
        .map_err(|error| error.to_string())?;
    assert!(!config.is_connect_managed());
    Ok(())
}
